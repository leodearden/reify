# Standard Library Boundary: Design Decisions

**Status:** Partial — covers module tree structure, `std.math`, `std.units`, `std.geometry`, `std.structural`, `std.ports`, `std.materials`, `std.tolerancing`. Remaining: `std.process`, `std.io`, `std.analysis`, `std.fields`, `std.determinacy`.  
**Version:** 0.1 — First crystallization from standard library boundary design sessions  
**Builds on:** `ontology-design-decisions.md` v0.1, `type-system-design-decisions.md` v0.1, `syntax-design-decisions.md` v0.1, `constraint-system-design-decisions.md` v0.1, `geometry-engine-design-decisions.md` v0.1, `evaluation-graph-design-decisions.md` v0.1, `evaluation-graph-completion-design-decisions.md` v0.2, `name-resolution-and-scoping-design-decisions.md` v0.1, `module-system-design-decisions.md` v0.1, `deferred-syntax-items-design-decisions.md` v0.1, `deferred-syntax-completion-design-decisions.md` v0.1

---

## 1. Design approach

The standard library boundary determines what lives in the language core (compiler-intrinsic), what ships in `std` (the standard library), what belongs in domain libraries (community/third-party), and what is auto-imported via the prelude.

**Guiding principles:**

1. **Framework over content.** `std` provides traits and type schemas; domain libraries provide implementations and data. `std` defines `MechanicalPort` (trait); a fastener library defines `NEMA17MountingInterface`. `std` defines `Material` (property schema); a materials library provides `Steel_304` with populated values.

2. **Coordination points in `std`.** Abstract traits that multiple independent libraries must agree on belong in `std`, because without shared definitions interoperability breaks. Port domain traits, material property traits, and tolerance types are coordination points.

3. **Dependency layering.** The module tree reflects dependency order: math depends on nothing; units/dimensions depend on math; geometry depends on both; ports and materials depend on geometry and units; processes depend on ports and materials; analysis and I/O depend on everything. Lower layers never import from higher layers.

4. **Concept-first organisation.** Modules are organised by concept/concern (`std.ports`, `std.materials`, `std.geometry`), not by engineering domain (`std.mechanical`, `std.electrical`). This keeps the tree flat, avoids the "where does a multi-domain thing go?" problem, and places cross-cutting concerns once.

---

## 2. Language-level additions identified

Two additions to the core language were identified during standard library design:

### 2.1 `enum` keyword

`enum` is a keyword for a simple sum type with compiler exhaustiveness checking on `match`. Too many cases in `std` require a small finite set of named alternatives (directionality, thread systems, fit types, hardness scales, material conditions, euler conventions, etc.) for these to be encoded as guarded structures.

```
enum Directionality { in, out, bidi }
enum FitType { Clearance, Transition, Interference }
enum HardnessScale { Rockwell_A, Rockwell_B, Rockwell_C, Brinell, Vickers, Shore_A, Shore_D }
```

### 2.2 `unit` keyword

`unit` is a keyword for declaring units that populate the unit namespace. Unit declarations are a special form — they populate the namespace that the tokeniser needs. The compiler reads `std.units.si` at a bootstrap stage before parsing user code.

```
unit m : Length
unit mm = 0.001m
unit N = kg * m / s^2
```

### 2.3 Chained comparisons

`a < b < c` desugars to `a < b && b < c`. Any comparison operators may chain. This is a significant readability improvement for range constraints, which are extremely common in engineering: `2mm < thickness < 10mm`, `0 < poissons_ratio < 0.5`.

Parser impact is modest (LL(2) at worst). Lexer impact is zero.

---

## 3. Three-tier boundary

### 3.1 Compiler-intrinsic (language core)

The compiler must understand these — they cannot be expressed in the language itself:

- Four entity types: `structure`, `occurrence`, `constraint`, `field`
- `fn` as a fifth declaration kind (not an entity type)
- `enum` as a sum type with exhaustiveness checking
- Type system: `Bool`, `Int`, `Real`, `String`, `Option<T>`, `List<T>`, `Set<T>`, `Map<T>`, `Range<T>`
- Physical quantity type system: 8-dimensional exponent vectors (7 SI + Angle), compile-time dimensional analysis, automatic unit conversion
- Affine space enforcement: `Point<N,Q>`, `Vector<N,Q>` with algebraic rules
- `Tensor<Rank,N,Q>`, `Matrix<M,N,Q>`, `Orientation<N>`, `Frame<N>`, `Transform<N>`
- Geometric entity types as opaque handles: `Solid`, `Shell`, `Surface`, `Curve`, `Point`, `PointCloud`
- Determinacy tracking: `undef`, `auto`, `determined`, `constrained`
- All keywords: `where`, `match`, `connect`, `chain`, `meta`, `let`, `param`, `port`, `sub`, `pub`, `fn`, `enum`, `unit`, `module`, `import`, `trait`, `self`, `purpose`
- The `@optimised` hook mechanism
- Module system, prelude injection, `#no_prelude`
- `purpose` desugaring (syntactic sugar for scoped constraints on specific entities)
- Unit literal syntax (number+unit, no space) and unit expression operators (`*`, `/`, `^`)

### 3.2 Standard library (`std`)

Ships with the language. Provides framework-level definitions, coordination traits, and universally needed utilities. Detailed contents specified in §§5–12.

### 3.3 Domain libraries (community/third-party)

Concrete implementations, data, and domain-specific logic:

- Material databases (Steel_304 properties, Al_6061_T6 properties, etc.)
- Specific manufacturing process definitions (5-axis CNC milling with specific machine parameters)
- Fastener catalogues (ISO 4017 hex bolts, DIN 912 socket caps)
- Kinematic joint libraries
- Standards compliance libraries (ISO 286 fit tables, ISO thread tables)
- Domain-specific DFM rule sets
- Specific interface standards (NEMA17, USB-C, etc.)

---

## 4. Module tree structure

```
std
├── prelude                 — auto-imported subset (see §4.1)
│
├── math                    — pure mathematics, no physics
│   ├── numeric             — abs, min, max, clamp, lerp, sqrt, pow, ...
│   ├── trig                — sin, cos, tan, asin, acos, atan, atan2, ...
│   ├── linalg              — dot, cross, normalize, magnitude, determinant, ...
│   └── complex             — Complex<Q>, conjugate, phase, ...
│
├── units                   — physical quantity infrastructure
│   ├── dimensions          — named dimension aliases (Area, Volume, Force, ...)
│   ├── si                  — SI base + derived units with all prefixes
│   ├── imperial            — minimal (in, ft, thou, lb, lbf, psi); community extends
│   └── constants           — pi, e, g, c, boltzmann, avogadro, ...
│
├── geometry                — geometric types, operations, properties
│   ├── constructors        — point3, vec3, orient_*, frame3, transform3, project
│   ├── primitive           — box, cylinder, sphere, cone, torus, half_space, ...
│   ├── compound            — tube, pipe, composed primitives
│   ├── boolean             — union, intersection, difference
│   ├── modify              — fillet, chamfer, shell, offset, draft, split, thicken
│   ├── sweep               — extrude, revolve, sweep, loft
│   ├── transform           — translate, rotate, scale, apply_transform
│   ├── pattern             — mirror, linear_pattern, circular_pattern, arbitrary_pattern
│   ├── query               — distance, angle, area, volume, contains, on, intersects,
│   │                         centroid, bounding_box, edges, faces, selectors
│   └── traits              — Geometry, Transformable, Plane, Axis, BoundingBox,
│                              Closed, Manifold, Orientable, Convex, Connected,
│                              Bounded, Watertight
│
├── structural              — structural behaviour traits
│   └── traits              — Physical, Rigid, Flexible, Elastic, Plastic, Sealed,
│                              ThermallyConductive, ElectricallyConductive
│
├── ports                   — port type hierarchy
│   ├── (mod.ri)            — Port, LocatedPort, RegionPort, Directionality
│   ├── mechanical          — MechanicalPort, ThreadedPort, ThreadSpec, MatingFace,
│   │                         MatingSurface, Bore, Shaft, MotivePort, RotaryPort,
│   │                         LinearPort, GuidePort, LinearGuidePort, RotaryGuidePort
│   ├── electrical          — ElectricalPort, PowerPort, SignalPort, PinPort
│   ├── thermal             — ThermalPort, ThermalContactPort
│   └── fluid               — FluidPort, PipedFluidPort
│
├── materials               — material property framework (traits, not databases)
│   ├── (mod.ri)            — Material, TemperatureDependent
│   ├── mechanical          — Elastic, Strong, Hard, FatigueRated, FractureTough,
│   │                         Ductile, ImpactResistant, Damping
│   ├── thermal             — ThermallyCharacterised, Refractory
│   ├── electrical          — ElectricallyCharacterised, Conductive, Insulating
│   ├── optical             — OpticallyCharacterised
│   └── chemical            — CorrosionResistant, Biocompatible
│
├── tolerancing             — GD&T and tolerance framework
│   ├── dimensional         — DimensionalTolerance, Fit, ISOToleranceGrade
│   ├── geometric           — GeometricTolerance, Conforms, Datum,
│   │                         Flatness, Straightness, Circularity, Cylindricity,
│   │                         Parallelism, Perpendicularity, Angularity,
│   │                         Position, Concentricity, Symmetry,
│   │                         CircularRunout, TotalRunout,
│   │                         ProfileOfSurface, ProfileOfLine
│   └── surface             — SurfaceFinish, SurfaceParameter, SurfaceDirection
│
├── process                 — manufacturing / transformation framework (TBD)
│   ├── (mod.ri)            — Process base trait, chain utilities
│   ├── categories          — Subtractive, Additive, Forming, Joining, Coating, HeatTreatment
│   └── dfm                 — DFM constraint framework patterns
│
├── io                      — boundary occurrences (TBD)
│   ├── (mod.ri)            — Output, Input (occurrence traits)
│   └── formats             — STEP, STL, 3MF, OBJ format trait definitions
│
├── analysis                — simulation integration (TBD)
│   ├── stress              — von_mises, principal_stresses, safety_factor
│   └── result              — simulation result import, field-from-external-data
│
├── fields                  — field manipulation utilities (TBD)
│   ├── interpolation       — lerp, bilinear, trilinear, rbf, kriging
│   └── spatial             — compose, threshold, clamp_field, remap_field, sample
│
└── determinacy             — determinacy predicates and purpose framework (TBD)
    ├── (mod.ri)            — determined(), constrained(), undetermined()
    ├── defaults            — default robustness objective
    └── purposes            — design, simulate, production_candidate, ...
```

### 4.1 Prelude contents

The prelude (`std.prelude`) is auto-imported into every module. It contains declarations that are universally needed. The prelude should be small (memorisable), stable (additions OK, removals are breaking changes), and universal (useful to a significant majority of modules).

**Prelude includes:**

- `std.math.numeric` — `abs`, `min`, `max`, `clamp`, `sqrt`, etc.
- `std.math.trig` — `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, etc.
- `std.math.linalg` — `dot`, `cross`, `normalize`, `magnitude`
- `std.units.dimensions` — all named dimension aliases
- `std.units.si` — all SI units with prefixes
- `std.units.constants.pi`
- `std.geometry.constructors` — `point3`, `vec3`, `orient_*`, `frame3`, `transform3`, `project`
- `std.ports.Port`, `std.ports.Directionality`
- `std.determinacy` predicates — `determined()`, `constrained()`, `undetermined()`

**Prelude excludes:**

- `std.geometry.boolean` and all geometry operations (Booleans, modify, sweep, pattern, query)
- `std.structural` traits
- Domain port types (`MechanicalPort`, `ElectricalPort`, etc.)
- `std.materials`
- `std.tolerancing`
- `std.process`
- `std.io`
- `std.analysis`
- `std.fields`

**Suppression:** `#no_prelude` pragma suppresses the implicit import.

---

## 5. `std.math`

### 5.1 `std.math.numeric`

```
fn abs<Q: Dimension>(x: Scalar<Q>) -> Scalar<Q>
fn min<Q: Dimension>(a: Scalar<Q>, b: Scalar<Q>) -> Scalar<Q>
fn max<Q: Dimension>(a: Scalar<Q>, b: Scalar<Q>) -> Scalar<Q>
fn clamp<Q: Dimension>(x: Scalar<Q>, lo: Scalar<Q>, hi: Scalar<Q>) -> Scalar<Q>
fn lerp<Q: Dimension>(a: Scalar<Q>, b: Scalar<Q>, t: Real) -> Scalar<Q>
fn remap(x: Real, from: Range<Real>, to: Range<Real>) -> Real
fn sqrt<Q: Dimension>(x: Scalar<Q^2>) -> Scalar<Q>     // dimension-aware — see §5.5
fn pow(base: Real, exp: Real) -> Real                    // dimensionless only for non-integer exp
fn log(x: Real) -> Real
fn log10(x: Real) -> Real
fn exp(x: Real) -> Real
fn sign<Q: Dimension>(x: Scalar<Q>) -> Int
fn floor(x: Real) -> Int
fn ceil(x: Real) -> Int
fn round(x: Real) -> Int
fn mod(a: Int, b: Int) -> Int
```

### 5.2 `std.math.trig`

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

Note: trig functions take `Angle` (not `Real`), enforcing the 8th-dimension distinction. `atan2` is generic over dimension — the dimension cancels in the ratio.

### 5.3 `std.math.linalg`

```
fn dot<N: Nat, Q1: Dimension, Q2: Dimension>(
    a: Vector<N, Q1>, b: Vector<N, Q2>
) -> Scalar<Q1 * Q2>

fn cross<Q1: Dimension, Q2: Dimension>(
    a: Vector<3, Q1>, b: Vector<3, Q2>
) -> Vector<3, Q1 * Q2>

fn normalize<N: Nat, Q: Dimension>(v: Vector<N, Q>) -> Vector<N, Dimensionless>
fn magnitude<N: Nat, Q: Dimension>(v: Vector<N, Q>) -> Scalar<Q>

fn determinant<N: Nat, Q: Dimension>(m: Matrix<N, N, Q>) -> Scalar<Q^N>
fn inverse<N: Nat, Q: Dimension>(m: Matrix<N, N, Q>) -> Matrix<N, N, Q^-1>
fn transpose<M: Nat, N: Nat, Q: Dimension>(m: Matrix<M, N, Q>) -> Matrix<N, M, Q>
fn outer<N: Nat, M: Nat, Q1: Dimension, Q2: Dimension>(
    a: Vector<N, Q1>, b: Vector<M, Q2>
) -> Matrix<N, M, Q1 * Q2>

fn trace<N: Nat, Q: Dimension>(m: Matrix<N, N, Q>) -> Scalar<Q>
fn eigenvalues<N: Nat, Q: Dimension>(m: Matrix<N, N, Q>) -> List<Scalar<Q>>
```

Note: `determinant` return type `Q^N` requires the type system to multiply a dimension exponent by a `Nat` — dependent typing over the dimension kind. Already supported by the type parameter model.

### 5.4 `std.math.complex`

```
structure def Complex<Q: Dimension> {
    param re : Scalar<Q>
    param im : Scalar<Q>
}

fn complex<Q: Dimension>(re: Scalar<Q>, im: Scalar<Q>) -> Complex<Q>
fn real<Q: Dimension>(z: Complex<Q>) -> Scalar<Q>
fn imag<Q: Dimension>(z: Complex<Q>) -> Scalar<Q>
fn conjugate<Q: Dimension>(z: Complex<Q>) -> Complex<Q>
fn complex_magnitude<Q: Dimension>(z: Complex<Q>) -> Scalar<Q>
fn phase<Q: Dimension>(z: Complex<Q>) -> Angle
```

Arithmetic between `Complex` values follows standard rules: `Complex<Q1> * Complex<Q2> -> Complex<Q1 * Q2>`. This is implemented via `@optimised` operator overloads.

### 5.5 Dimensional `sqrt`

`sqrt` is dimension-aware: `sqrt(4m^2)` yields `2m`. This requires the compiler to verify that all exponents in the input dimension vector are even, then halve them. This is a compiler intrinsic, not expressible in the language's type system alone.

`pow` with non-integer exponents is restricted to dimensionless quantities in v0.1. `pow` with integer literal exponents on dimensioned quantities works through the type system (repeated multiplication).

`magnitude` uses dimensional `sqrt` internally — `magnitude(vec3(3m, 4m, 0m))` returns `5m`.

---

## 6. `std.units`

### 6.1 `std.units.dimensions`

Named dimension aliases. These are type aliases — `Force` and `Mass * Length / Time^2` are the same type.

```
dimension Area = Length^2
dimension Volume = Length^3
dimension Velocity = Length / Time
dimension Acceleration = Length / Time^2
dimension AngularVelocity = Angle / Time
dimension AngularAcceleration = Angle / Time^2
dimension Frequency = Time^-1
dimension Force = Mass * Acceleration
dimension Torque = Force * Length / Angle
dimension Energy = Force * Length
dimension Power = Energy / Time
dimension Pressure = Force / Area
dimension Stress = Pressure
dimension Strain = Dimensionless
dimension Density = Mass / Volume
dimension MomentOfInertia = Mass * Length^2
dimension SectionModulus = Length^3
dimension SecondMomentOfArea = Length^4
dimension Stiffness = Force / Length
dimension RotationalStiffness = Torque / Angle
dimension Viscosity = Pressure * Time
dimension KinematicViscosity = Area / Time
dimension Voltage = Power / Current
dimension Resistance = Voltage / Current
dimension Capacitance = Current * Time / Voltage
dimension Inductance = Voltage * Time / Current
dimension Charge = Current * Time
dimension MagneticFlux = Voltage * Time
dimension MagneticFluxDensity = MagneticFlux / Area
dimension ElectricField = Voltage / Length
dimension ThermalConductivity = Power / (Length * Temperature)
dimension SpecificHeat = Energy / (Mass * Temperature)
dimension HeatFlux = Power / Area
dimension TemperatureDiff = Temperature
dimension Luminance = Luminosity / Area
dimension LuminousFlux = Luminosity * Angle^2
dimension Illuminance = LuminousFlux / Area
```

`Stress = Pressure` is an intentional alias — same exponent vector, different name for readability. The type system treats them identically.

### 6.2 `std.units.si`

SI base and derived units with every defined SI prefix (quecto through quetta). Unit declarations use the `unit` keyword.

```
// Length — base unit and all SI prefixes
unit m : Length
unit Qm = 1e30m       // quettametre
unit Rm = 1e27m        // ronnametre
unit Ym = 1e24m        // yottametre
unit Zm = 1e21m        // zettametre
unit Em = 1e18m        // exametre
unit Pm = 1e15m        // petametre
unit Tm = 1e12m        // terametre
unit Gm = 1e9m         // gigametre
unit Mm = 1e6m         // megametre
unit km = 1e3m
unit hm = 1e2m
unit dam = 10m
unit dm = 0.1m
unit cm = 0.01m
unit mm = 1e-3m
unit um = 1e-6m        // micrometre
unit nm = 1e-9m
unit pm = 1e-12m
unit fm = 1e-15m
unit am = 1e-18m
unit zm = 1e-21m
unit ym = 1e-24m
unit rm = 1e-27m
unit qm = 1e-30m

// Mass — base unit and all SI prefixes (base is kg, so prefixes apply to g)
unit kg : Mass
unit g = 0.001kg
unit mg = 1e-6kg
unit ug = 1e-9kg
unit ng = 1e-12kg
unit tonne = 1000kg

// Time
unit s : Time
unit ms = 1e-3s
unit us = 1e-6s
unit ns = 1e-9s
unit ps = 1e-12s
unit fs = 1e-15s
unit min = 60s
unit hr = 3600s

// Current
unit A : Current
unit kA = 1e3A
unit mA = 1e-3A
unit uA = 1e-6A
unit nA = 1e-9A

// Temperature
unit K : Temperature
unit mK = 1e-3K
unit uK = 1e-6K
unit degC : Temperature offset 273.15K

// Angle
unit rad : Angle
unit mrad = 1e-3rad
unit urad = 1e-6rad
unit deg = (pi / 180) * rad
unit arcmin = deg / 60
unit arcsec = arcmin / 60
unit rev = 2 * pi * rad

// Amount of substance
unit mol : Amount
unit mmol = 1e-3mol
unit umol = 1e-6mol
unit nmol = 1e-9mol

// Luminous intensity
unit cd : Luminosity

// Derived — Force
unit N = kg * m / s^2
unit kN = 1e3N
unit MN = 1e6N
unit mN = 1e-3N
unit uN = 1e-6N

// Derived — Energy
unit J = N * m
unit kJ = 1e3J
unit MJ = 1e6J
unit GJ = 1e9J
unit mJ = 1e-3J
unit uJ = 1e-6J
unit eV = 1.602176634e-19J

// Derived — Power
unit W = J / s
unit kW = 1e3W
unit MW = 1e6W
unit GW = 1e9W
unit mW = 1e-3W
unit uW = 1e-6W
unit nW = 1e-9W

// Derived — Pressure
unit Pa = N / m^2
unit kPa = 1e3Pa
unit MPa = 1e6Pa
unit GPa = 1e9Pa
unit mPa = 1e-3Pa
unit bar = 1e5Pa
unit mbar = 1e-3bar

// Derived — Electrical
unit V = W / A
unit kV = 1e3V
unit mV = 1e-3V
unit uV = 1e-6V
unit ohm = V / A
unit kohm = 1e3ohm
unit Mohm = 1e6ohm
unit mohm = 1e-3ohm
unit S = A / V              // siemens (conductance)
unit mS = 1e-3S
unit F = A * s / V
unit mF = 1e-3F
unit uF = 1e-6F
unit nF = 1e-9F
unit pF = 1e-12F
unit H = V * s / A
unit mH = 1e-3H
unit uH = 1e-6H
unit nH = 1e-9H
unit Wb = V * s
unit T = Wb / m^2
unit mT = 1e-3T
unit uT = 1e-6T
unit nT = 1e-9T

// Derived — Frequency
unit Hz = 1 / s
unit kHz = 1e3Hz
unit MHz = 1e6Hz
unit GHz = 1e9Hz
unit THz = 1e12Hz
unit mHz = 1e-3Hz

// Derived — Angular velocity
unit rpm = rev / min
unit rad_per_s = rad / s

// Derived — Viscosity
unit Pa_s = Pa * s

// Derived — Luminous
unit lm = cd * rad^2          // lumen
unit lx = lm / m^2            // lux

// Derived — Radioactivity / dose (minimal set)
unit Bq = 1 / s               // becquerel
unit Gy = J / kg              // gray
unit Sv = J / kg              // sievert (same dimension as gray, different quantity)
```

### 6.3 `std.units.imperial`

Minimal imperial unit set. Community libraries extend as needed.

```
unit in = 25.4mm
unit ft = 12 * in
unit thou = 0.001 * in
unit yd = 3 * ft
unit lb = 0.45359237kg
unit oz = lb / 16
unit lbf = lb * 9.80665m/s^2
unit psi = lbf / in^2
unit ksi = 1000psi
unit degF : Temperature offset 255.372K scale 5/9
unit fl_oz = 29.5735295625e-6 * m^3
unit gal = 128 * fl_oz
```

Note: `degF` requires both offset and scale in the unit declaration syntax — `(T_F − 32) × 5/9 + 273.15 = T_K`. The `offset` + `scale` keywords in unit declarations need formal specification.

### 6.4 `std.units.constants`

```
let pi : Real = 3.14159265358979323846
let e : Real = 2.71828182845904523536
let g : Acceleration = 9.80665m/s^2
let c : Velocity = 299792458m/s
let boltzmann : Energy / Temperature = 1.380649e-23J/K
let avogadro : Amount^-1 = 6.02214076e23/mol
let planck : Energy * Time = 6.62607015e-34J*s
let stefan_boltzmann : Power / (Area * Temperature^4) = 5.670374419e-8W/(m^2*K^4)
let vacuum_permittivity : Capacitance / Length = 8.8541878128e-12F/m
let vacuum_permeability : Inductance / Length = 1.25663706212e-6H/m
let gas_constant : Energy / (Amount * Temperature) = 8.314462618J/(mol*K)
let elementary_charge : Charge = 1.602176634e-19A*s
```

---

## 7. `std.geometry`

### 7.1 `std.geometry.constructors`

In the prelude.

```
fn point2<Q: Dimension>(x: Scalar<Q>, y: Scalar<Q>) -> Point2<Q>
fn point3<Q: Dimension>(x: Scalar<Q>, y: Scalar<Q>, z: Scalar<Q>) -> Point3<Q>
fn vec2<Q: Dimension>(x: Scalar<Q>, y: Scalar<Q>) -> Vector2<Q>
fn vec3<Q: Dimension>(x: Scalar<Q>, y: Scalar<Q>, z: Scalar<Q>) -> Vector3<Q>

fn orient_axis_angle(axis: Vector3<Dimensionless>, angle: Angle) -> Orientation<3>
fn orient_quaternion(w: Real, x: Real, y: Real, z: Real) -> Orientation<3>
fn orient_euler(convention: EulerConvention, a: Angle, b: Angle, c: Angle) -> Orientation<3>
fn orient_basis(
    x: Vector3<Dimensionless>,
    y: Vector3<Dimensionless>,
    z: Vector3<Dimensionless>
) -> Orientation<3>
fn orient_look_at(forward: Vector3<Dimensionless>, up: Vector3<Dimensionless>) -> Orientation<3>
let orient_identity : Orientation<3>

fn frame3(origin: Point3<Length>, basis: Orientation<3>) -> Frame<3>
let frame3_identity : Frame<3>

fn transform3(rotation: Orientation<3>, translation: Vector3<Length>) -> Transform<3>
let transform3_identity : Transform<3>

fn project<N: Nat, Q: Dimension>(value: Point<N, Q>, to: Frame<N>) -> Point<N, Q>
fn project<N: Nat, Q: Dimension>(value: Vector<N, Q>, to: Frame<N>) -> Vector<N, Q>

enum EulerConvention { XYZ, XZY, YXZ, YZX, ZXY, ZYX }
```

### 7.2 `std.geometry.primitive`

3D solids, 2D filled regions, and curves.

```
// 3D solids
fn box(x: Length, y: Length, z: Length) -> Solid
fn box_centered(x: Length, y: Length, z: Length) -> Solid
fn cylinder(radius: Length, height: Length) -> Solid
fn cylinder_centered(radius: Length, height: Length) -> Solid
fn cone(base_radius: Length, top_radius: Length, height: Length) -> Solid
fn sphere(radius: Length) -> Solid
fn torus(major_radius: Length, minor_radius: Length) -> Solid
fn wedge(x: Length, y: Length, z: Length, ltx: Length) -> Solid
fn half_space(plane: Plane) -> Solid

// 2D filled regions
fn rectangle(width: Length, height: Length) -> Surface
fn circle(radius: Length) -> Surface
fn polygon(vertices: List<Point2<Length>>) -> Surface
fn ellipse(semi_major: Length, semi_minor: Length) -> Surface

// Curves
fn line_segment<N: Nat>(from: Point<N, Length>, to: Point<N, Length>) -> Curve
fn arc(center: Point3<Length>, radius: Length, start: Angle, end: Angle) -> Curve
fn helix(radius: Length, pitch: Length, height: Length) -> Curve
fn interp<N: Nat>(points: List<Point<N, Length>>) -> Curve
fn bezier<N: Nat>(control_points: List<Point<N, Length>>) -> Curve
fn nurbs<N: Nat>(
    control_points: List<Point<N, Length>>,
    weights: List<Real>,
    knots: List<Real>,
    degree: Int
) -> Curve
fn nurbs_surface(
    control_points: List<List<Point3<Length>>>,
    weights_u: List<Real>, weights_v: List<Real>,
    knots_u: List<Real>, knots_v: List<Real>,
    degree_u: Int, degree_v: Int
) -> Surface
```

**Design note — `half_space` and `Bounded`:** `half_space` produces an unbounded `Solid`. This means `Solid` does not imply `Bounded`. Operations requiring bounded inputs (volume computation, meshing, export) require the `Bounded` trait explicitly. Boolean operations work on unbounded inputs — `intersection(half_space(p1), half_space(p2))` gives a dihedral wedge, still unbounded. This is a change from the geometry engine doc's implicit assumption.

### 7.3 `std.geometry.compound`

Composed primitives — not primitive geometry, but common enough to standardise.

```
fn tube(outer_radius: Length, inner_radius: Length, height: Length) -> Solid
fn pipe(path: Curve, radius: Length) -> Solid
```

### 7.4 `std.geometry.boolean`

```
fn union(a: Solid, b: Solid) -> Solid
fn union_all(solids: List<Solid>) -> Solid
fn intersection(a: Solid, b: Solid) -> Solid
fn intersection_all(solids: List<Solid>) -> Solid
fn difference(a: Solid, b: Solid) -> Solid

fn union(a: Surface, b: Surface) -> Surface
fn intersection(a: Surface, b: Surface) -> Surface
fn difference(a: Surface, b: Surface) -> Surface
```

### 7.5 `std.geometry.modify`

```
fn fillet(body: Solid, edges: List<Curve>, radius: Length) -> Solid
fn fillet_all(body: Solid, radius: Length) -> Solid
fn chamfer(body: Solid, edges: List<Curve>, distance: Length) -> Solid
fn chamfer_asymmetric(body: Solid, edges: List<Curve>, d1: Length, d2: Length) -> Solid

fn shell(body: Solid, thickness: Length) -> Solid
fn shell_open(body: Solid, thickness: Length, open_faces: List<Surface>) -> Solid

fn offset_solid(body: Solid, distance: Length) -> Solid
fn offset_surface(surf: Surface, distance: Length) -> Surface
fn offset_curve(curve: Curve, distance: Length) -> Curve
fn offset_curve(curve: Curve, distance: Length, reference: Surface) -> Curve
fn offset_curve(curve: Curve, distance: Length, direction: Vector3<Dimensionless>) -> Curve

fn draft(
    body: Solid, faces: List<Surface>,
    pull_direction: Vector3<Dimensionless>, angle: Angle
) -> Solid

fn split(body: Solid, tool: Surface) -> List<Solid>
fn split(body: Solid, tool: Solid) -> List<Solid>
fn split(surf: Surface, tool: Surface) -> List<Surface>
fn split(surf: Surface, tool: Solid) -> List<Surface>
fn split(surf: Surface, tool: Curve) -> List<Surface>
fn split(curve: Curve, tool: Surface) -> List<Curve>
fn split(curve: Curve, tool: Point3<Length>) -> List<Curve>

fn thicken(surf: Surface, thickness: Length) -> Solid
fn thicken_asymmetric(surf: Surface, t_above: Length, t_below: Length) -> Solid
```

Split pattern: a tool of dimension N splits geometry of dimension ≥ N. Offset operations move geometry along the local normal. 2D curve offset is unambiguous; 3D curve offset requires a reference surface or direction.

### 7.6 `std.geometry.sweep`

```
fn extrude(profile: Surface, direction: Vector3<Length>) -> Solid
fn extrude_to(profile: Surface, target: Surface) -> Solid
fn extrude_symmetric(profile: Surface, direction: Vector3<Dimensionless>, distance: Length) -> Solid

fn revolve(profile: Surface, axis: Curve, angle: Angle) -> Solid
fn revolve_full(profile: Surface, axis: Curve) -> Solid

fn sweep(profile: Surface, path: Curve) -> Solid
fn sweep_guided(profile: Surface, path: Curve, guide: Curve) -> Solid

fn loft(profiles: List<Surface>) -> Solid
fn loft_guided(profiles: List<Surface>, guides: List<Curve>) -> Solid
```

### 7.7 `std.geometry.transform`

```
fn translate<G>(body: G, displacement: Vector3<Length>) -> G
    where G: Transformable

fn rotate<G>(body: G, axis: Axis, angle: Angle) -> G
    where G: Transformable

fn rotate<G>(body: G, orientation: Orientation<3>) -> G
    where G: Transformable

fn rotate_around<G>(
    body: G, point: Point3<Length>,
    axis: Vector3<Dimensionless>, angle: Angle
) -> G
    where G: Transformable

fn scale<G>(body: G, factor: Real) -> G
    where G: Transformable

fn scale<G>(body: G, factors: Vector3<Dimensionless>) -> G
    where G: Transformable

fn apply_transform<G>(body: G, transform: Transform<3>) -> G
    where G: Transformable
```

Note: `scale` is non-rigid — it does not compose with `Transform<3>` (which is rotation + translation only).

### 7.8 `std.geometry.pattern`

```
fn mirror<G>(body: G, plane: Plane) -> G
    where G: Transformable

fn linear_pattern<G>(body: G, direction: Vector3<Length>, count: Int) -> List<G>
    where G: Transformable

fn circular_pattern<G>(body: G, axis: Axis, count: Int) -> List<G>
    where G: Transformable

fn linear_pattern_2d<G>(
    body: G,
    dir1: Vector3<Length>, count1: Int,
    dir2: Vector3<Length>, count2: Int
) -> List<G>
    where G: Transformable

fn arbitrary_pattern<G>(body: G, transforms: List<Transform<3>>) -> List<G>
    where G: Transformable
```

`arbitrary_pattern` takes `List<Transform<3>>` rather than `List<Point<3, Length>>` — transforms include orientation at each location, not just position. Patterns return `List` for per-instance constraints; compose with `union_all` for a merged solid.

### 7.9 `std.geometry.query`

```
// Distance
fn distance<G1, G2>(a: G1, b: G2) -> Length
    where G1: Geometry, G2: Geometry

fn closest_point<G>(target: G, from: Point3<Length>) -> Point3<Length>
    where G: Geometry

// Angular
fn angle(a: Vector3<Dimensionless>, b: Vector3<Dimensionless>) -> Angle
fn angle_between_surfaces(a: Surface, b: Surface) -> Angle

// Containment
fn contains(body: Solid, point: Point3<Length>) -> Bool
fn on<G>(point: Point3<Length>, target: G) -> Bool
    where G: Geometry
fn intersects(a: Solid, b: Solid) -> Bool

// Measurement
fn area(s: Surface) -> Area
fn area(s: Solid) -> Area
fn volume(s: Solid) -> Volume
fn length(c: Curve) -> Length
fn perimeter(s: Surface) -> Length

// Mass properties
fn centroid(s: Solid) -> Point3<Length>
fn center_of_mass(s: Solid, density: Density) -> Point3<Length>
fn moment_of_inertia(s: Solid, density: Density, axis: Axis) -> MomentOfInertia
fn bounding_box<G>(g: G) -> BoundingBox
    where G: Geometry

// Surface / curve queries
fn normal(s: Surface, at: Point2<Dimensionless>) -> Vector3<Dimensionless>
fn curvature(s: Surface, at: Point2<Dimensionless>) -> Real
fn curvature(c: Curve, at: Real) -> Real

// Topology queries
fn edges(s: Solid) -> List<Curve>
fn faces(s: Solid) -> List<Surface>
fn adjacent_faces(s: Solid, edge: Curve) -> List<Surface>
fn shared_edges(s: Solid, f1: Surface, f2: Surface) -> List<Curve>

// Selectors
fn edges_by_length(s: Solid, range: Range<Length>) -> List<Curve>
fn faces_by_area(s: Solid, range: Range<Area>) -> List<Surface>
fn faces_by_normal(
    s: Solid, direction: Vector3<Dimensionless>, tolerance: Angle
) -> List<Surface>
fn edges_parallel_to(
    s: Solid, direction: Vector3<Dimensionless>, tolerance: Angle
) -> List<Curve>
fn edges_at_height(
    s: Solid, axis: Vector3<Dimensionless>, height: Length, tolerance: Length
) -> List<Curve>
```

### 7.10 `std.geometry.traits`

```
/// Supertype for all geometric entity types
trait Geometry {}

/// Has geometry that can be repositioned via Transform
trait Transformable {}

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

    let size : Vector3<Length> = max - min
    let center : Point3<Length> = point3(
        (min.x + max.x) / 2,
        (min.y + max.y) / 2,
        (min.z + max.z) / 2
    )
}

fn plane_xy(height: Length) -> Plane
fn plane_xz(height: Length) -> Plane
fn plane_yz(height: Length) -> Plane
fn axis_x() -> Axis
fn axis_y() -> Axis
fn axis_z() -> Axis

// Geometric property traits
trait Closed {}
trait Manifold {}
trait Orientable {}
trait Convex {}
trait Connected {}
trait Bounded {}
trait Watertight : Closed + Manifold {}
```

`Solid` no longer implies `Bounded` (see §7.2 design note on `half_space`).

---

## 8. `std.structural`

Structural behaviour traits — physical characteristics that cut across specific part types.

### 8.1 `std.structural.traits`

```
/// A structure with physical geometry and mass
trait Physical {
    param geometry : Solid
    param material : Material
    let mass : Mass = volume(geometry) * material.density
    let centroid : Point3<Length> = centroid(geometry)
}

/// A physical structure that is rigid under expected loading
trait Rigid : Physical {
    let moment_of_inertia : Tensor<2, 3, MomentOfInertia>
}

/// A structure that deforms meaningfully under expected loading
trait Flexible : Physical {
    param stiffness_model : StiffnessModel
}

/// Deforms under load, returns to original shape when load removed
trait Elastic : Flexible {}

/// Deforms permanently under sufficient load
trait Plastic : Flexible {
    param yield_point : Pressure
}

/// A structure that conducts heat
trait ThermallyConductive : Physical {}

/// A structure that conducts electricity
trait ElectricallyConductive : Physical {}

/// A structure that can be sealed against fluid passage
trait Sealed {
    param seal_rating : Pressure
}
```

**Design rationale:** This set is deliberately thin. These traits cover the main axes of physical behaviour that different domain libraries need to agree on. Domain-specific structural traits (`Composite`, `Thermoplastic`, `Electrochemical`, `Optical`, etc.) belong in domain libraries.

`Physical` requires geometry. Structures that don't implement `Physical` can exist as topological/constraint-only entities — substanceless connectors, abstract interfaces.

---

## 9. `std.ports`

### 9.1 `std.ports` (mod.ri)

```
trait Port {
    param direction : Directionality = bidi
}

enum Directionality { in, out, bidi }

trait LocatedPort : Port {
    param frame : Frame<3>
}

trait RegionPort : LocatedPort {
    param region : Geometry
    let frame : Frame<3> = region.reference_frame
}
```

`RegionPort` implements `LocatedPort` — a region has an implicit frame derived from its geometry. The `region` parameter takes any `Geometry` type (surface, volume, curve, point), not just `Surface`.

### 9.2 `std.ports.mechanical`

```
trait MechanicalPort : LocatedPort {
    param max_load : Force = undef
    param max_torque : Torque = undef
}

// --- Mating interfaces ---

trait MatingFace : MechanicalPort {
    param contact_area : Area = undef
    param surface_finish : Real = undef
    param flatness : Length = undef
}

trait MatingSurface : MechanicalPort {
    param contact_region : Geometry = undef
    param surface_finish : Real = undef
}

// --- Cylindrical interfaces ---

trait Bore : MechanicalPort {
    param diameter : Length
    param depth : Length = undef
    param fit : FitType = undef
}

trait Shaft : MechanicalPort {
    param diameter : Length
    param length : Length = undef
    param fit : FitType = undef
}

enum FitType { Clearance, Transition, Interference }

// --- Threaded interfaces ---

trait ThreadedPort : MechanicalPort {
    param thread_spec : ThreadSpec
    param thread_length : Length = undef
}

structure def ThreadSpec {
    param system : ThreadSystem
    param nominal_diameter : Length
    param pitch : Length
    param class_fit : ThreadClass = undef
    param tightening_direction : ThreadTighteningDirection = Clockwise
    param thread_form : Geometry = undef

    let clearance_hole : Length          // from system tables
    let tap_drill : Length               // from system tables
    let minor_diameter : Length          // computed
    let pitch_diameter : Length          // computed
}

enum ThreadSystem { ISO_Metric, ISO_Metric_Fine, UNC, UNF }
enum ThreadClass { Class_6g6H, Class_4g6H }
enum ThreadTighteningDirection { Clockwise, Anticlockwise }

// --- Motion transmission ---

trait MotivePort : MechanicalPort {}

trait RotaryPort : MotivePort {
    param max_speed : AngularVelocity = undef
    param max_torque : Torque = undef
    param axis : Axis
}

trait LinearPort : MotivePort {
    param max_speed : Velocity = undef
    param max_force : Force = undef
    param stroke : Length = undef
    param axis : Axis
}

// --- Motion constraint (guides) ---

trait GuidePort : MechanicalPort {
    param degrees_of_freedom : Int
}

trait LinearGuidePort : GuidePort {
    param axis : Axis
    param stroke : Length = undef
    param max_load : Force = undef
    constraint degrees_of_freedom == 1
}

trait RotaryGuidePort : GuidePort {
    param axis : Axis
    param max_radial_load : Force = undef
    param max_axial_load : Force = undef
    constraint degrees_of_freedom == 1
}
```

### 9.3 `std.ports.electrical`

```
trait ElectricalPort : Port {
    param voltage_rating : Voltage = undef
    param current_rating : Current = undef
}

trait PowerPort : ElectricalPort {
    param power_rating : Power = undef
}

trait SignalPort : ElectricalPort {
    param signal_type : SignalType = undef
    param impedance : Resistance = undef
}

enum SignalType { Analog, Digital, PWM, Differential }

trait PinPort : ElectricalPort + LocatedPort {
    param pin_id : String = undef
}
```

Electrical ports do not require geometry by default — a `SignalPort` in early-stage system design may have no physical location yet.

### 9.4 `std.ports.thermal`

```
trait ThermalPort : Port {
    param temperature : Temperature = undef
    param heat_flux : HeatFlux = undef
    param thermal_resistance : Scalar<Temperature / Power> = undef
}

trait ThermalContactPort : ThermalPort + RegionPort {
    param contact_area : Area = undef
    param contact_conductance : Scalar<Power / (Area * Temperature)> = undef
}
```

### 9.5 `std.ports.fluid`

```
trait FluidPort : Port {
    param pressure_range : Range<Pressure> = undef
    param flow_rate_range : Range<Scalar<Volume / Time>> = undef
    param fluid_type : FluidType = undef
}

enum FluidType { Liquid, Gas, TwoPhase }

trait PipedFluidPort : FluidPort + LocatedPort {
    param inner_diameter : Length
    param connection_type : PipeConnectionType = undef
}

enum PipeConnectionType { Threaded, Flanged, Compression, PushFit, Welded }
```

Fluid port ratings are `Range` rather than scalar — both minimum and maximum bounds matter (minimum flow for cooling, maximum pressure for burst).

---

## 10. `std.materials`

Material property schema — traits defining what properties a material can have. Actual material data belongs in domain libraries.

### 10.1 `std.materials` (mod.ri)

```
trait Material {
    param density : Density
    param name : String = undef
}

trait TemperatureDependent {
    param reference_temperature : Temperature = 293.15K
}
```

For v0.1: scalar properties with optional reference temperature. Field-valued properties (e.g. `Field<Temperature, Pressure>` for temperature-dependent modulus) are a clear v0.2 feature.

### 10.2 `std.materials.mechanical`

```
trait Elastic : Material {
    param youngs_modulus : Pressure
    param poissons_ratio : Real
    param shear_modulus : Pressure = undef

    constraint 0 < poissons_ratio < 0.5
}

trait Strong : Material {
    param yield_strength : Pressure = undef
    param ultimate_tensile_strength : Pressure = undef
    param compressive_strength : Pressure = undef

    constraint ultimate_tensile_strength >= yield_strength
        where determined(yield_strength) && determined(ultimate_tensile_strength)
}

trait Hard : Material {
    param hardness_value : Real
    param hardness_scale : HardnessScale
}

enum HardnessScale { Rockwell_A, Rockwell_B, Rockwell_C, Brinell, Vickers, Shore_A, Shore_D }

trait FatigueRated : Material {
    param fatigue_limit : Pressure = undef
    param fatigue_strength_at : Pressure = undef
    param fatigue_cycles : Int = undef
}

trait FractureTough : Material {
    param fracture_toughness : Scalar<Pressure * Length^(1/2)>
}

trait Ductile : Material {
    param elongation_at_break : Real = undef
    param reduction_of_area : Real = undef
}

trait ImpactResistant : Material {
    param charpy_impact : Energy = undef
    param izod_impact : Energy = undef
}

trait Damping : Material {
    param loss_factor : Real = undef
}
```

Note: `FractureTough.fracture_toughness` has dimension `Pressure * Length^(1/2)` — the first non-integer dimension exponent in a user-facing `std` type. The type system supports rational exponents.

### 10.3 `std.materials.thermal`

```
trait ThermallyCharacterised : Material {
    param thermal_conductivity : ThermalConductivity
    param specific_heat : SpecificHeat
    param thermal_expansion : Scalar<Temperature^-1> = undef
    param melting_point : Temperature = undef
    param max_service_temperature : Temperature = undef
    param glass_transition : Temperature = undef
}

trait Refractory : ThermallyCharacterised {
    constraint max_service_temperature >= 1500degC
        where determined(max_service_temperature)
}
```

### 10.4 `std.materials.electrical`

```
trait ElectricallyCharacterised : Material {
    param resistivity : Scalar<Resistance * Length> = undef
    param dielectric_constant : Real = undef
    param dielectric_strength : Scalar<Voltage / Length> = undef
    param magnetic_permeability : Real = undef
}

trait Conductive : ElectricallyCharacterised {
    constraint resistivity < 1e-4 ohm*m
        where determined(resistivity)
}

trait Insulating : ElectricallyCharacterised {
    constraint resistivity > 1e6 ohm*m
        where determined(resistivity)
    constraint determined(dielectric_strength)
}
```

### 10.5 `std.materials.optical`

```
trait OpticallyCharacterised : Material {
    param refractive_index : Real = undef
    param absorption_coefficient : Scalar<Length^-1> = undef
    param transmittance : Real = undef
    param reference_thickness : Length = undef
}
```

### 10.6 `std.materials.chemical`

```
trait CorrosionResistant : Material {
    param corrosion_class : CorrosionClass = undef
}

enum CorrosionClass { C1, C2, C3, C4, C5 }

trait Biocompatible : Material {
    param biocompatibility_class : BiocompatibilityClass = undef
}

enum BiocompatibilityClass { USP_Class_I, USP_Class_VI, ISO_10993 }
```

### 10.7 Design notes

**No material categories.** `std` does not define `trait Metal`, `trait Polymer`, `trait Ceramic`, `trait Composite`. These are fuzzy taxonomic categories (is a metallic glass a metal? is CFRP a polymer?) that belong in a materials taxonomy library. DFM checks that need "is this a metal?" can check for specific property combinations.

**Constraint-carrying traits.** `Conductive`, `Insulating`, and `Refractory` carry constraints on property values. This enables transitive checking: a process trait like `Solderable` can require `Conductive`, which transitively requires `resistivity < 1e-4 Ω·m`.

---

## 11. `std.tolerancing`

### 11.1 `std.tolerancing.dimensional`

```
structure def DimensionalTolerance {
    param nominal : Length
    param upper_deviation : Length
    param lower_deviation : Length

    let upper_limit : Length = nominal + upper_deviation
    let lower_limit : Length = nominal + lower_deviation
    let tolerance_band : Length = upper_deviation - lower_deviation

    constraint upper_deviation >= lower_deviation
}

fn symmetric_tolerance(nominal: Length, deviation: Length) -> DimensionalTolerance
fn limit_tolerance(upper: Length, lower: Length) -> DimensionalTolerance

structure def Fit {
    param hole_tolerance : DimensionalTolerance
    param shaft_tolerance : DimensionalTolerance
    param fit_type : FitCategory

    let max_clearance : Length = hole_tolerance.upper_limit - shaft_tolerance.lower_limit
    let min_clearance : Length = hole_tolerance.lower_limit - shaft_tolerance.upper_limit

    constraint fit_type == Clearance implies min_clearance > 0mm
    constraint fit_type == Interference implies max_clearance < 0mm
    where fit_type == Transition {
        constraint max_clearance > 0mm
        constraint min_clearance < 0mm
    }
}

enum FitCategory { Clearance, Transition, Interference }

structure def ISOToleranceGrade {
    param grade : Int
    param nominal_range : Range<Length>
    let tolerance_value : Length          // from standards library tables
}
```

### 11.2 `std.tolerancing.geometric`

GD&T types are structures that compute tolerance zone geometry. The universal `Conforms` constraint checks containment.

```
trait GeometricTolerance {
    param tolerance_value : Length
    param feature : Geometry
    param material_condition : MaterialCondition = RFS
    let nominal_zone : Geometry           // computed by each specific tolerance type
}

enum MaterialCondition { MMC, LMC, RFS }

structure def Datum {
    param label : String
    param feature : Geometry
}

// --- Form tolerances (no datum reference) ---

trait FormTolerance : GeometricTolerance {}

structure def Flatness : FormTolerance {
    param tolerance_value : Length
    param feature : Surface
    let nominal_zone : Geometry = slab(fit_plane(feature), tolerance_value)
}

structure def Straightness : FormTolerance {
    param tolerance_value : Length
    param feature : Curve
}

structure def Circularity : FormTolerance {
    param tolerance_value : Length
    param feature : Curve
}

structure def Cylindricity : FormTolerance {
    param tolerance_value : Length
    param feature : Surface
}

// --- Orientation tolerances (require datum reference) ---

trait OrientationTolerance : GeometricTolerance {
    param datum_refs : List<Datum>
}

structure def Parallelism : OrientationTolerance {
    param tolerance_value : Length
    param feature : Geometry
    param datum_refs : List<Datum>
}

structure def Perpendicularity : OrientationTolerance {
    param tolerance_value : Length
    param feature : Geometry
    param datum_refs : List<Datum>
}

structure def Angularity : OrientationTolerance {
    param tolerance_value : Length
    param feature : Geometry
    param datum_refs : List<Datum>
    param nominal_angle : Angle
}

// --- Location tolerances ---

trait LocationTolerance : GeometricTolerance {
    param datum_refs : List<Datum>
}

structure def Position : LocationTolerance {
    param tolerance_value : Length
    param feature : Geometry
    param datum_refs : List<Datum>
    param material_condition : MaterialCondition = RFS
    let nominal_zone : Geometry = tolerance_cylinder(
        nominal_location(feature, datum_refs),
        tolerance_value / 2
    )
}

structure def Concentricity : LocationTolerance {
    param tolerance_value : Length
    param feature : Geometry
    param datum_refs : List<Datum>
}

structure def Symmetry : LocationTolerance {
    param tolerance_value : Length
    param feature : Geometry
    param datum_refs : List<Datum>
}

// --- Runout tolerances ---

structure def CircularRunout : GeometricTolerance {
    param tolerance_value : Length
    param feature : Surface
    param datum_refs : List<Datum>
}

structure def TotalRunout : GeometricTolerance {
    param tolerance_value : Length
    param feature : Surface
    param datum_refs : List<Datum>
}

// --- Profile tolerances ---

structure def ProfileOfSurface : GeometricTolerance {
    param tolerance_value : Length
    param feature : Surface
    param datum_refs : List<Datum> = []
    param unilateral : Bool = false
}

structure def ProfileOfLine : GeometricTolerance {
    param tolerance_value : Length
    param feature : Curve
    param datum_refs : List<Datum> = []
    param unilateral : Bool = false
}

// --- Universal conformance check ---

constraint def Conforms {
    param subject : Geometry
    param zone : GeometricTolerance

    let effective_zone : Geometry = match zone.material_condition {
        MMC => expand_zone(zone, subject, MMC)
        LMC => expand_zone(zone, subject, LMC)
        RFS => zone.nominal_zone
    }

    constraint contains(effective_zone, subject)
}
```

Helper functions (`slab`, `fit_plane`, `tolerance_cylinder`, `nominal_location`, `expand_zone`) are `@optimised` library functions in `std.tolerancing.geometric`.

### 11.3 `std.tolerancing.surface`

```
structure def SurfaceFinish {
    param parameter : SurfaceParameter
    param value : Length
    param direction : SurfaceDirection = undef
    param process : String = undef
}

enum SurfaceParameter { Ra, Rz, Rq, Rt, Rp, Rv, Rsk, Rku }
enum SurfaceDirection { Parallel, Perpendicular, Crossed, Multidirectional, Circular, Radial }

fn require_finish(feature: Surface, finish: SurfaceFinish) -> Constraint
```

### 11.4 Design notes

**Tolerance stack-up analysis** (RSS, worst-case, Monte Carlo) is deferred to v0.2. It requires the assembly graph, geometric query, and statistical computation to all be working. The framework (declaring stack-up paths) may live in `std.tolerancing`; the methods (Monte Carlo simulation) belong in `std.analysis` or a dedicated library.

**Application pattern.** Tolerances are instantiated as sub-structures within a design, alongside the geometry they constrain:

```
structure def BracketWithTolerances : Rigid {
    param body : Solid = ...

    sub datum_A : Datum { label = "A", feature = bottom_face }

    sub mounting_flatness : Flatness {
        feature = faces_by_normal(body, vec3(0, 0, 1), 1deg)[0]
        tolerance_value = 0.05mm
    }

    sub hole_position : Position {
        feature = mounting_hole
        tolerance_value = 0.1mm
        datum_refs = [datum_A]
        material_condition = MMC
    }

    constraint Conforms(mounting_hole, hole_position)
}
```

---

## 12. Updates to prior documents

### 12.1 `evaluation-graph-completion-design-decisions.md` §5.5

`Export` and `Import` occurrence traits are renamed to **`Output`** and **`Input`** respectively, to avoid collision with the module-system `import` keyword. An `Output` occurrence consumes a structure/field and emits it to the outside world. An `Input` occurrence produces a structure/field from external data.

### 12.2 `geometry-engine-design-decisions.md`

`Solid` no longer implies `Bounded`. The `Bounded` trait is explicitly required by operations that need bounded inputs (volume computation, meshing, export). This enables `half_space` and other unbounded geometric primitives.

### 12.3 Language additions

- `enum` keyword for simple sum types with compiler exhaustiveness checking.
- `unit` keyword for unit declarations.
- Chained comparison expressions: `a < b < c` desugars to `a < b && b < c`.

---

## 13. Open questions for subsequent sessions

### 13.1 Remaining `std` modules

`std.process`, `std.io`, `std.analysis`, `std.fields`, and `std.determinacy` are not yet specified. These are scheduled for the next session.

### 13.2 `enum` formal specification

`enum` is identified as a language-level keyword but not yet formally specified in the syntax or type system design documents. Needs: declaration syntax, exhaustiveness checking rules on `match`, and interaction with the trait system (can enums implement traits? can traits require enum parameters?).

### 13.3 `unit` declaration formal specification

The `unit` keyword, its declaration syntax, and the bootstrap ordering (compiler reads `std.units.si` before parsing user code) need formal specification. The `offset` and `scale` modifiers for temperature units (`degC`, `degF`) need syntax design.

### 13.4 Chained comparison formal specification

Grammar rule, desugaring, and interaction with the expression precedence table need specification.

### 13.5 `Geometry` supertrait

The `trait Geometry {}` that all geometric entity types implement needs integration with the type system design — specifically, how `Solid`, `Surface`, `Curve`, `Point`, and `PointCloud` are declared as implementing it, and whether this is compiler-level or library-level.

### 13.6 Tolerance stack-up framework

Deferred to v0.2 but flagged as an important capability for manufacturing readiness assessment.

---

## 14. Summary of key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Organising principle | Concept-first (`std.ports`, `std.geometry`) not domain-first (`std.mechanical`) | Flat tree; cross-cutting concerns addressed once; avoids "where does multi-domain go?" |
| `std` content boundary | Framework traits and type schemas; no concrete parts, materials, or processes | Coordination points in `std`; implementations in domain libraries |
| Prelude scope | Math, trig, linalg, dimensions, SI units, pi, geometry constructors, Port, determinacy predicates | Small, stable, universal |
| `enum` | Language-level keyword with exhaustiveness checking | Too many finite alternative sets in `std` for guarded-structure encoding |
| `unit` | Language-level keyword with bootstrap ordering | Tokeniser needs unit namespace populated before parsing user code |
| Chained comparisons | `a < b < c` desugars to conjunction | Engineering readability for range constraints |
| Dimensional `sqrt` | Compiler intrinsic; halves even exponents | Too common in engineering to leave awkward (`magnitude`, stress intensity, etc.) |
| `half_space` / unbounded `Solid` | `Solid` no longer implies `Bounded` | Half-spaces are fundamental Boolean operands; `Bounded` required explicitly where needed |
| `Geometry` supertrait | All geometric entity types implement `Geometry` | Enables generic port regions, generic queries, generic patterns |
| `RegionPort` extends `LocatedPort` | Region's geometry provides implicit frame | A region has a location; pretending otherwise creates busywork |
| `RegionPort.region` type | `Geometry` (not `Surface`) | Ports can be volumes, surfaces, curves, or points |
| Output/Input rename | Replaces Export/Import for boundary occurrences | Avoids collision with module-level `import` keyword |
| Material schema | Property traits in `std`; data in domain libraries | Coordination on property types; actual values are domain content |
| No material categories | No `Metal`/`Polymer`/`Ceramic` in `std` | Fuzzy boundaries; DFM can check property combinations instead |
| GD&T model | Tolerance types are structures computing zone geometry; `Conforms` checks containment | Tolerances have identity and metadata; checking is constraint-based |
| Tolerance stack-up | Deferred to v0.2 | Requires assembly graph + statistical analysis infrastructure |
| `RigidMechanical` | Removed for v0.1 | `Rigid` is sufficient; marker trait wasn't earning its keep |
| Mechanical motion ports | `MotivePort`/`RotaryPort`/`LinearPort` for power; `GuidePort` hierarchy for constraint | Distinct concepts: power transmission vs motion constraint |
| Fluid port ratings | `Range<Pressure>` and `Range<Scalar<Volume/Time>>` | Both bounds matter (min flow for cooling, max pressure for burst) |
| ThreadSpec | In `std` with `thread_form : Geometry` | Coordination type for library interop; geometry enables profile reconstruction |

---

*Document generated from standard library boundary design sessions. Covers `std.math`, `std.units`, `std.geometry`, `std.structural`, `std.ports`, `std.materials`, `std.tolerancing`. Remaining modules (`std.process`, `std.io`, `std.analysis`, `std.fields`, `std.determinacy`) to be resolved in subsequent sessions.*
