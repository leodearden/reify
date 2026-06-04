# Reify Language Specification

**Version:** 0.1
**Date:** 2026-03-13
**Status:** Draft -- synthesized from 16 design decision documents and design review resolutions

---

## 1. Introduction

### 1.1 Language Name and Purpose

**Reify** is a text-based domain-specific language for engineering design. The name means "to make real," describing the language's central activity: a design begins as an undetermined specification and is progressively reified into a fully determined, manufacturable artifact. Source files use the `.ri` extension.

### 1.2 Target Domains

Reify targets mechanical and mechatronic design, simulation, and manufacturing -- including assembly, additive manufacturing, and CAM. It is designed to serve as the single unified framework from abstract sketch through to manufacturing-ready specification.

### 1.3 Design Philosophy

**Core principles (priority-ordered for syntax):**

1. **Regularity** -- every entity type follows the same declaration shape; every member kind uses the same syntax pattern; minimize special forms.
2. **Concision** -- common things short, rare things can be longer, no ceremony for simple cases.
3. **Explicitness** -- structure visible in text, no significant whitespace, no implicit scoping rules requiring indentation counting.
4. **Readability at a glance** -- scanning a file immediately reveals entities, types, connections. Keywords over symbols for semantically important constructs.
5. **Parseability** -- LL(k) with small k. No ambiguities requiring unbounded lookahead. LLMs generate more reliably when grammar is predictable.

**Broader principles:**

- **Elegance:** Fundamental simplicity, orthogonality, composability, modularity.
- **Expressive power:** Simple things are simple; complex things are no harder than necessary.
- **Fluidity:** Full spectrum from abstract sketch to manufacturing-ready spec in one unified framework.
- **Human-LLM co-authoring:** Primary authoring mode. Syntax must be concise, unambiguous, and structurally regular for reliable LLM generation while remaining natural for human engineers.
- **Clean-slate design:** Not built on existing standards, but designed to bridge to them (STEP, FMI, 3MF, etc.).
- **Minimal core, rich libraries:** Core is small, powerful, and general. Domain complexity (GD&T, DFM rules, kinematic joints, material databases) belongs in community-driven libraries.

**Style family:** Curly-brace, declarative, Modelica-meets-USD. Rust-influenced expression syntax. Explicitly not C++, not Python, not Lisp.

---

## 2. Lexical Structure

### 2.1 Identifiers

- `snake_case` -- values, parameters, ports, sub-structures, fields, modules
- `PascalCase` -- types, traits, entity definitions
- `SCREAMING_SNAKE` -- compile-time constants (convention only, not enforced by grammar)
- Start with a letter or underscore, followed by letters, digits, underscores.
- Unicode letters permitted (for internationalization of parameter names, material names, etc.).
- Keywords are ASCII-only.

```
IDENT       ::= [a-zA-Z_][a-zA-Z0-9_]*
TYPE_IDENT  ::= [A-Z][a-zA-Z0-9_]*
```

### 2.2 Comments

```
// Line comment -- to end of line
/* Block comment -- nests correctly */
/// Doc comment -- attached to the following declaration
```

Block comments nest, unlike C. `/* outer /* inner */ still in outer */` is valid. Rationale: small implementation cost for significant usability gain (commenting out code that already contains comments).

Doc comments (`///`) are for tooling-generated documentation -- API docs, hover text, inline help. They are distinct from `meta` blocks (see Section 4.8).

### 2.3 Numeric Literals

```
42              // Int
3.14            // Real
1.5e-3          // Real, scientific notation
1_000_000       // Underscores as visual separators (Int or Real)
0xFF            // Hex integer
0b1010          // Binary integer
```

No implicit coercion from `Real` to `Int`. `Int` promotes to `Real` implicitly (see Section 3.1).

### 2.4 String Literals

```
"hello world"           // Standard string
"line one\nline two"    // Escape sequences: \n \t \\ \" \uXXXX
```

No string interpolation in the core language. String interpolation is a display/templating concern.

### 2.5 Boolean Literals

```
true    false
```

### 2.6 Quantity Literals

A physical quantity is a numeric literal immediately followed by a unit expression, with **no space** between number and unit.

```
5mm                 // Length: 5 millimetres
3.2kN               // Force: 3.2 kilonewtons
45deg               // Angle: 45 degrees
293.15K             // Temperature: 293.15 kelvin
2.5e-3m             // Length: 0.0025 metres (scientific notation)
25USD               // Money: 25 US dollars
```

`5mm` is a quantity literal. `5 mm` is the integer `5` followed by the identifier `mm` -- a syntax error or misinterpretation. Bare numbers are dimensionless: `3.14` is `Real` (dimensionless). To get a dimensioned quantity, a unit must be written. There is no "default unit system."

### 2.7 Unit Expressions

Units compose with `*`, `/`, and `^` in postfix position after a number:

```
5kN*m               // Torque: kilonewton-metres
2.1kg/m^3           // Density
9.81m/s^2           // Acceleration
1.2e-6m^2/s         // Kinematic viscosity
25USD/kg            // Cost per unit mass
```

**Precedence within unit expressions:** `^` binds tightest, then `*` and `/` left-to-right. Parentheses available for disambiguation:

```
5kg*m/s^2           // = kg . m . s^-2  (Force) -- ^ binds to s only
5kg*m/(s^2)         // Same thing, explicit
5(kg*m/s)^2         // = kg^2 . m^2 . s^-2 -- parenthesized unit raised to power
```

The full unit table is defined in the standard library (`std.units`), not the grammar. The grammar defines the syntax for unit expressions; the standard library populates the unit namespace.

### 2.8 Range Literals

```
2mm..5mm            // Range<Length>: closed interval [2mm, 5mm]
0deg..<360deg       // Half-open: [0 deg, 360 deg)
>2mm                // Open lower bound: (2mm, infinity)
<=100MPa            // Closed upper bound: (-infinity, 100MPa]
```

Range syntax uses `..` (inclusive) and `..<` (exclusive upper bound). Single-sided ranges use comparison operators as prefixes.

### 2.9 Special Values

```
undef               // "Not yet decided" -- default state of unassigned parameters
auto                // "System, figure this out" -- delegation to constraint solver
some(value)         // Option<T>: present value
none                // Option<T>: absent value
```

`undef`, `auto`, `some`, `none` are keywords, not identifiers (§2.10). `undef` can appear anywhere a value expression is expected. `some(value)` and `none` are the constructors for `Option<T>`.

`auto` and `auto(free)` are valid only at a **binding site** — a position where a named declaration slot directly receives its value:

- a `param` default (`param wall_thickness : Length = auto`),
- a sub-instance parameter override (`sub b : Bearing { bore = auto }`),
- a `let` binding (`let m : Length = auto`),
- a structure-construction named argument (`Bolt(length: auto)`),
- a connect-parameter assignment (`connect a -> b { gain = auto }`).

`auto` is **not** a general expression operand. It may not appear as a function-call argument, inside an arithmetic or logical expression, in a `constraint`/`minimize`/`maximize` body, in a field `source`, or in a collection literal — these are parse errors. To bound a solver-delegated value, attach a constraint to the binding rather than embedding `auto` in an expression: write `length = auto, length > 2mm`, not `length = auto + 2mm`. (Reconciled 2026-05-26 with the grammar, which admits `auto` only at binding sites; see `docs/prds/auto-binding-site-positions.md`. The prior "anywhere a value expression is expected" wording over-promised relative to the parser.)

**`auto` modes:**

- **Strict `auto` (default):** Resolution requires the resolved value is well-determined -- either uniquely determined by constraints or uniquely optimal under the applicable objective. If the system cannot determine a unique best resolution, strict `auto` is an error.
- **Free `auto`:** Explicit opt-in for exploration. Returns a feasible solution and triggers a warning that the result is not uniquely determined.

```
param wall_thickness : Length = auto           // Strict (default)
param wall_thickness : Length = auto(free)     // Free -- exploration mode
```

### 2.10 Keywords (Complete Post-Review List)

**Entity kinds:** `structure`, `occurrence`, `constraint`, `field`

**Declaration keywords:** `def`, `param`, `port`, `sub`, `let`, `type`, `fn`, `pub`, `module`, `import`, `trait`, `purpose`, `enum`, `unit`

**Connection keywords:** `connect`, `chain`

**Control/guard keywords:** `where`, `match`, `if`, `then`, `else`

**Logical keywords:** `and`, `or`, `not`, `implies`, `forall`, `exists`

**Direction keywords:** `in`, `out`

> **Disambiguation: `in` as keyword vs unit.** `in` is both a direction keyword and the imperial unit for inches (= 25.4mm). Since quantity literals require no space between number and unit, the lexer distinguishes them: `5in` is a quantity literal (5 inches), while bare `in` is always the keyword. The sequence `5 in` (with space) is the integer `5` followed by the keyword `in`.

**Value literals/keywords:** `true`, `false`, `undef`, `auto`, `some`, `none`

**Optimization keywords:** `minimize`, `maximize`

**Metadata:** `meta`

**Self-reference:** `self`

**Collection literal prefixes:** `set`, `map`

**Module/import-related:** `as`

**Removed keywords (not part of v0.1):** `derived` (replaced by `let`), `require` (replaced by `constraint` + determinacy predicates), `dimension` (replaced by `type` for type aliases)

**Not keywords (standard library functions):** `determined`, `constrained`, `undetermined`, `partially_determined`, `point3`, `vec3`, `point2`, `vec2`, `project`, `geo_equiv`

---

## 3. Types

### 3.1 Primitive Types

| Type     | Description |
|----------|-------------|
| `Bool`   | Predicates, flags, gating of optional sub-structures |
| `Int`    | Counts, indices, discrete quantities. Promotes to `Real` implicitly but not reverse |
| `Real`   | Dimensionless real number. Alias for `Scalar<Dimensionless>` (Section 3.3.1). Precision is an implementation concern |
| `String` | Names, labels, identifiers, human-readable descriptions |

`Int` and `Real` are separate types. A bolt count is categorically different from a wall thickness. The type system catches continuous/discrete confusion. Precision (float32/float64/arbitrary) is abstracted away -- the toolchain decides based on context.

**Subtyping rule:** `Int` promotes to `Real` implicitly, but `Real` does NOT promote to `Int`.

**`Real` and the dimensional type system:** `Real` is identical to `Scalar<Dimensionless>` -- they are the same type. `Dimensionless` is the dimension whose exponent vector is all zeros. Bare real numbers participate in dimensional arithmetic naturally: `3.14 * 5mm` produces `Scalar<Length>` because `Scalar<Dimensionless> * Scalar<Length> = Scalar<Length>` (dimension exponent vectors add).

**`Int` in dimensional arithmetic:** When `Int` appears in arithmetic with a dimensioned quantity, it promotes to `Real` (= `Scalar<Dimensionless>`) first. Thus `3 * 5mm` evaluates as `Scalar<Dimensionless> * Scalar<Length>` = `15mm`. When both operands are `Int`, no promotion occurs -- `Int` arithmetic stays `Int` (except division; see Section 5.1). An `Int` literal immediately followed by a unit (`5mm`) is a quantity literal (Section 2.6), not a promotion -- it directly produces a `Scalar<Length>`.

### 3.2 Physical Quantity Types and Dimensional Analysis

**Core model:** Dimensions are part of the type. Units are part of the literal syntax and value representation. Two quantities with the same dimension and different units are the SAME type. The type checker operates on dimensions; unit conversion is automatic.

**Dimension representation:** A vector of rational exponents over 10 base dimensions (7 SI + Angle + SolidAngle + Money):

```
[Length, Mass, Time, Current, Temperature, Amount, Luminosity, Angle, SolidAngle, Money]

Length       = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0]
Force        = [1, 1, -2, 0, 0, 0, 0, 0, 0, 0]   // M*L*T^-2
Pressure     = [-1, 1, -2, 0, 0, 0, 0, 0, 0, 0]  // M*L^-1*T^-2
Torque       = [1, 1, -2, 0, 0, 0, 0, -1, 0, 0]  // M*L*T^-2*Angle^-1 (distinct from Energy)
CostPerMass  = [0, -1, 0, 0, 0, 0, 0, 0, 0, 1]   // Money*Mass^-1
```

Multiplication adds exponent vectors. Division subtracts. Checked at compile time with zero runtime cost.

**Angle as 8th base dimension:** Angles are dimensionless in SI (radians = m/m), but treating them as dimensionless is a known error source. Torque/energy confusion (both `N*m`) is the canonical example. Adding Angle as a base dimension catches `torque + energy` as a type error. Cost: trig functions need explicit typing (`sin : Angle -> Dimensionless`).

**SolidAngle as 9th base dimension:** Solid angles are dimensionless in SI (steradians = m²/m²), but tracking them separately prevents confusion between planar-angle and solid-angle quantities. Enables correct typing of luminous intensity (`cd = lm/sr`), beam-pattern calculations, and radiation-pattern integrals. Cost: spherical functions need explicit typing (`steradians -> Dimensionless`).

**Money as 10th base dimension:** Monetary units (`USD`, `GBP`, `EUR`, etc.) are declared with the `unit` keyword. All monetary values within a project use constant conversion factors. Time-varying exchange rates are out of scope. Enables expressions like `25USD/kg` for cost estimation. Money composes with physical dimensions via multiplication/division like any other dimension.

**Named dimension aliases:** Type aliases, not new types -- `Force` and `Mass * Length / Time^2` are the same type.

```
type Force    = Mass * Length / Time^2
type Pressure = Force / Length^2
type Density  = Mass / Length^3
```

**Temperature offsets:** `degC` and `degF` are offset units. The distinction between absolute temperature and temperature difference matters:

```
param max_temp : Temperature = 150degC       // Absolute: 423.15 K
param delta_t  : TemperatureDiff = 20degC     // Difference: 20 K
```

Type system distinguishes:
- `Temperature + TemperatureDiff -> Temperature` (valid)
- `Temperature - Temperature -> TemperatureDiff` (valid)
- `Temperature + Temperature -> type error` (invalid)

**Standard named dimension aliases (35 in `std.units.dimensions`):**

`Dimensionless` (all-zero exponent vector, identical to `Real`), `Area`, `Volume`, `Velocity`, `Acceleration`, `AngularVelocity`, `AngularAcceleration`, `Frequency`, `Force`, `Torque`, `Energy`, `Power`, `Pressure`, `Stress` (= `Pressure`), `Strain` (= `Dimensionless`), `Density`, `MomentOfInertia`, `SectionModulus`, `SecondMomentOfArea`, `Stiffness`, `RotationalStiffness`, `Viscosity`, `KinematicViscosity`, `Voltage`, `Resistance`, `Capacitance`, `Inductance`, `Charge`, `MagneticFlux`, `MagneticFluxDensity`, `ElectricField`, `ThermalConductivity`, `SpecificHeat`, `HeatFlux`, `TemperatureDiff`, `Luminance`, `LuminousFlux`, `Illuminance`

### 3.3 Geometric Types

#### 3.3.1 Algebraic Geometric Types

**Point/Vector distinction (affine space):** Points and vectors are different types. Points are positions (affine space elements). Vectors are displacements (vector space elements).

Algebraic rules enforced by the type system:
- `Point - Point -> Vector` (valid)
- `Point + Vector -> Point` (valid)
- `Vector + Vector -> Vector` (valid)
- `Point + Point` -> type error

**Parameterization:** Geometric types are parameterized by spatial dimensionality and quantity:

```
Point<N: Nat, Q: Dimension>    // Position. Q typically Length.
Vector<N: Nat, Q: Dimension>   // Displacement / physical vector.
```

Type aliases for common cases:
```
type Point2<Q> = Point<2, Q>
type Point3<Q> = Point<3, Q>
type Vector2<Q> = Vector<2, Q>
type Vector3<Q> = Vector<3, Q>
```

Geometric types carry physical dimensions. `Point3<Length>` = position in physical space. `Vector3<Force>` = force vector. Unifies dimensional analysis with geometry.

**Scalar type:**

```
Scalar<Q: Dimension>           // Dimensioned number -- independent type
```

`Scalar<Q>` is an independent type representing a single dimensioned value. Unlike `Vector` and higher-rank tensors, `Scalar` does not carry a spatial dimensionality parameter -- a rank-0 tensor has no spatial indices, so `N` is meaningless.

`Scalar<Dimensionless>` is identical to `Real` (see Section 3.1).

**Tensor conversion:** `Scalar<Q>` converts implicitly to `Tensor<0, N, Q>` for any `N`, and vice versa. This allows scalars to participate seamlessly in generic tensor expressions.

**Tensor type:**

```
Tensor<Rank: Nat, N: Nat, Q: Dimension>
```

All indices range over the same spatial dimension N. Transforms covariantly/contravariantly under coordinate changes.

Subtype/alias relationships:
- `Vector<N, Q> = Tensor<1, N, Q>` (rank-1 alias)
- `Scalar<Q>` is an independent type; converts to/from `Tensor<0, N, Q>` for any `N`
- `Real = Scalar<Dimensionless>` (convenience alias; see Section 3.1)
- `Point<N, Q>` is NOT a tensor -- separate affine-space type

Tensor symmetry is expressed via trait (`Symmetric`), not type parameter. Allows implementation to optimize storage (6 vs. 9 components for symmetric 3x3) based on trait satisfaction.

**Matrix type:**

```
Matrix<M: Nat, N: Nat, Q: Dimension>
```

Separate type for general M x N rectangular arrays. `Tensor<2, N, Q>` implicitly converts to `Matrix<N, N, Q>` but NOT the reverse (an arbitrary square matrix is not necessarily a physics tensor with correct transformation properties).

**Orientation:**

Opaque type representing rotation in N-dimensional space. Language does NOT commit to a representation. Construction from any common representation:

```
Orientation.from_quaternion(w, x, y, z)
Orientation.from_axis_angle(axis, angle)
Orientation.from_euler(convention, a, b, c)
Orientation.from_basis(x_axis, y_axis, z_axis)
Orientation.identity
```

**Frame and Transform:**

```
Frame<N>:
    origin : Point<N, Length>
    basis  : Orientation<N>

Transform<N>:
    rotation    : Orientation<N>
    translation : Vector<N, Length>
```

Key semantics:
- `Transform` is always rigid (rotation + translation). Non-rigid maps (scaling, shearing) are a separate type.
- Sub-structure placement is a `Transform` from child frame to parent frame.
- Global position is computed by composing Transforms up the containment tree.
- No implicit global frame. All coordinates relative to parent.
- Ports expose Frames (when geometrically located). Connections constrain Transforms between Frames.

**Realized (v0.6).** Declarative `at` placement and compose-up-the-tree auto-surfacing are implemented at the geometry level. Sub-placement syntax (`at` pose clause, `aux` modifier) is documented in §4.7; the auto-surfacing idiom and when to retain a manual lift are in §8.3. See also [docs/prds/v0_6/sub-placement-and-surfacing.md](docs/prds/v0_6/sub-placement-and-surfacing.md).

**Geometric values carry their frame:** Geometric values (`Point3`, `Vector3`, etc.) carry their coordinate frame as part of their runtime representation. Frame is not part of the type but tracked by the runtime. When defined within a structure, the frame is the structure's local coordinate frame.

**Geometric collections share frames efficiently:** Geometric collections (point clouds, meshes) store coordinates relative to a single frame -- the containing structure's local coordinate frame. One frame per collection, not per element. When an individual point is extracted, it acquires the collection's frame.

#### 3.3.2 Opaque Geometry Types

Core geometric entity types are opaque handles -- designers cannot inspect vertices or control points. They work through operations: `union(a,b)`, `fillet(solid, edge, radius)`, `distance(p, surface)`.

| Type         | Description |
|--------------|-------------|
| `Solid`      | Closed region of 3D space (closed, not necessarily bounded) |
| `Shell`      | Connected set of faces bounding a region |
| `Surface`    | 2D manifold in 3D space |
| `Curve`      | 1D manifold in 2D/3D space |
| `Point`      | 0-dimensional position (as a geometric entity) |
| `PointCloud`  | Unordered point collection |

Geometric property traits: `Closed`, `Manifold`, `Orientable`, `Convex`, `Connected`, `Bounded`, `Watertight` (= `Closed + Manifold`). `Solid` no longer implies `Bounded` -- operations requiring bounded inputs require the `Bounded` trait explicitly.

The `Geometry` supertrait is the parent of all geometric entity types. `Transformable` trait marks types that can be spatially transformed.

### 3.4 Collection Types

| Type          | Purpose |
|---------------|---------|
| `List<T>`     | Ordered sequence (bolt patterns, process step lists, point clouds) |
| `Set<T>`      | Unordered unique collection (material options, feature sets) |
| `Map<K, V>`   | Key-value mapping (material property tables, parameter lookups) |
| `Range<T>`    | Bounded interval -- `Range<Length>` for `2mm..5mm` tolerance ranges |
| `Option<T>`   | Explicit optionality (present or absent). Distinct from `undef` |

**`Option<T>` vs `undef`:** `Option` is a type-level statement about existence (the value may or may not be present); `undef` is a determinacy state (the value has not been decided yet). A parameter is always present -- it may just be `undef`. See Section 9.2.8 for the four-way distinction between `some(value)`, `some(undef)`, `none`, and `undef` of type `Option<T>`.

**`Option<Option<T>>`** is a valid type. `some(some(x))`, `some(none)`, and `none` are all distinguishable via pattern matching.

**Collection operations (v0.1):**

- **`List<T>`:** `count`, `sum` (numeric T), `map`, `filter`, `fold`, `all`, `any`, `contains`, `[i]` indexing, `generate(n, fn)`, `concat`.
- **`Set<T>`:** `count`, `contains`, `union`, `intersection`, `difference`. Iteration via `forall`/`exists`.
- **`Map<K,V>`:** `[key]` lookup, `keys`, `values`, `count`, `contains_key`. Iteration via `forall`/`exists` over entries.
- **`Range<T>`:** `contains`, `lower`, `upper`, `span` (upper - lower).

**Out-of-bounds / missing key:** Evaluation-graph-level failure (see Section 9), not a language-level exception.

**Empty collections:**

- `List.generate(0, fn)` is valid and returns `[]` (empty list of the appropriate element type).
- `[].sum` requires element type context. When the element type is known (e.g., the list is typed as `List<Length>`), the result is the additive identity for that dimension (`0mm` for `Length`, `0N` for `Force`, etc.). Without type context, it is a type error.
- `[].count` is `0`. `set{}.count` is `0`. `map{}.count` is `0`.
- `forall x in []: P(x)` evaluates to `true` (vacuous truth -- standard mathematical convention).
- `exists x in []: P(x)` evaluates to `false` (vacuous falsity -- standard mathematical convention).
- Empty set literal `set{}` and empty map literal `map{}` require type context (e.g., `let s : Set<Material> = set{}`).

**Counted sub-structures use `List<T>` uniformly.** The `[n]` syntax for counted sub-structure arrays is removed. Declaration: `sub vents : List<Vent>` plus `constraint vents.count == vent_count`. The runtime recognizes `count == N` constraints on `List<Structure>` sub-declarations as structure-controlling, triggering schema re-elaboration. v0.1 uses positional indexing (`vents[0]`, `vents[1]`, ...).

**Collection literals:**

```
[1, 2, 3]                       // List literal
set{a, b, c}                    // Set literal -- prefix avoids block ambiguity
map{"key" => value, "k2" => v2} // Map literal -- prefix avoids paren ambiguity
```

### 3.5 Field Type

```
Field<D, C>
```

`D` = domain type, `C` = codomain type. Fields are compiler-intrinsic opaque mathematical objects (like geometry). Examples:

```
Field<Point3<Length>, Scalar<Temperature>>         // Temperature distribution
Field<Point3<Length>, Vector3<Force>>               // Force field
Field<Real, Scalar<Length>>                         // 1D profile
Field<Point3<Length>, Tensor<2, 3, Pressure>>      // Stress tensor field
```

Composition is type-safe: composing `Field<A,B>` with `Field<B,C>` yields `Field<A,C>`.

### 3.6 Complex Numbers

`Complex<Q>` is a standard library type (not a core primitive). Structured type (pair of Q values) with arithmetic defined over it. Dimensional system composes naturally: `Complex<Impedance>` has real and imaginary parts both in impedance units.

```
Complex<Q>:
    re : Scalar<Q>
    im : Scalar<Q>
```

### 3.7 Function Types

```
Point3<Length> -> Scalar<Temperature>       // Spatial temperature field
(Length, Length) -> Bool                     // Binary predicate
```

Arrow `->` for function types. Tuple types with `(A, B, C)` for multi-parameter domains.

### 3.8 Enum Types

v0.1 enums are C-style: simple named alternatives with no associated data.

```
enum Directionality { In, Out, Bidi }
enum FitType { Clearance, Transition, Interference }
```

`Option<T>` with `some(value)` / `none` remains compiler-intrinsic, not an enum.

**Exhaustiveness:** `match` on an enum must cover all variants or use `_` wildcard.

### 3.9 Type Parameters and Inference

**Two kinds of parameters:**

- **Type parameters** -- parameterize over types. Resolved at definition time (compile time). Written in angle brackets.
- **Value parameters** -- parameterize over values. Resolved at instantiation time. Exist along the determinacy spectrum. Written in the body.

```
structure def FlexibleCoupling<DriverPort: RotaryPort, DrivenPort: RotaryPort> {
    param max_torque : Torque
    param max_misalignment : Angle
}
```

**Type parameter bounds (three kinds):**

- **Trait bound:** `T: SomeTrait` -- T must implement SomeTrait
- **Kind bound:** `N: Nat`, `Q: Dimension` -- T must be a member of a built-in kind
- **Composite bound:** `T: TraitA + TraitB` -- multiple trait requirements

**Type parameter defaults:**

```
structure def Fastener<HeadStyle: HeadType = Hex> { ... }
```

**`auto` for type parameters:** Valid. Meaning: "I want this to be some specific type; system, figure out which one."

```
sub bearing1 : Bearing<auto: Seal> { bore_diameter = 25mm }
```

**Type parameter `auto` resolution:** The search space is all concrete types visible at the instantiation site that satisfy the bound. Resolution proceeds as follows:

1. **Enumerate candidates** -- all types satisfying the trait/kind bound that are in scope (imported or fully qualified).
2. **Filter by feasibility** -- instantiate with each candidate and check whether the resulting constraints are satisfiable (using the same mechanism as value `auto`).
3. **Select** -- if exactly one candidate is feasible, use it (strict `auto`). If multiple are feasible, strict `auto` is an error; `auto(free)` selects one deterministically (lexicographic by fully qualified name).

Type parameter `auto` is resolved at elaboration time, before value parameter resolution begins. If resolution fails, the diagnostic reports the bound, the candidates considered, and why each was rejected or why selection was ambiguous.

The candidate pool is capped at **10**. If more than 10 in-scope types satisfy the bound, resolution errors with `E_AUTO_TYPE_PARAM_POOL_OVERFLOW` listing the alphabetically-first 10 and asks the user to disambiguate. Lexicographic tiebreak by fully qualified name (FQN) applies both to candidate ordering and to `auto(free)` selection. Multiple `auto:` type-params in one definition resolve in declared order; no cross-parameter backtracking in v0.1.

See `docs/auto-type-param-resolution.md` for the complete algorithm, diagnostic codes, and worked example. Cross-parameter backtracking is deferred to v0.2 per `docs/prds/v0_2/auto-resolution-backtracking.md`.

**Type inference:** Conservative. Infer type parameters when context unambiguously determines them. Never infer value parameters -- the determinacy model handles "not yet specified" via `undef`/`auto`/constrained/determined.

**Limited dependent typing:** Value parameters of type `Int` and `Bool` can appear in type-level positions (collection sizes, conditional presence gating, array dimensions). This is a targeted set of rules, not a general dependent type theory.

### 3.10 Determinacy and Types

Determinacy is tracked orthogonally, not baked into types. Parameter types are written as plain `Length`, `Force`, etc. Determinacy (`undef` / constrained / `auto` / determined) is a property of the parameter tracked by the design system, not part of the type.

---

## 4. Declarations

### 4.1 Entity Declarations (Uniform Shape)

All entity declarations follow:

```
<entity_kind> def <Name><TypeParams>? <TraitList>? <WhereClause>? {
    <members>
}
```

Where `<entity_kind>` is one of: `structure`, `occurrence`, `constraint`, `field`.

The four entity types have identity, determinacy state, and evaluation graph presence.

#### 4.1.1 Structure Declarations

```
structure def Bracket<M: Material> : Rigid {
    param thickness : Length
    param width : Length = 50mm
    param material : M

    port mount_face : MechanicalPort {
        direction = in
        frame = Frame3 { origin = point3(0mm, 0mm, 0mm) }
    }

    port load_face : MechanicalPort {
        direction = out
        frame = Frame3 { origin = point3(0mm, width, 0mm) }
    }

    sub rib : Rib { height = thickness * 0.8 }

    let volume = thickness * width * width
    let mass = volume * material.density

    constraint thickness > 1mm
    constraint thickness < width / 2
}
```

Structures compose spatially (containment of sub-structures). They are immutable within the design system. No Part/Assembly distinction -- a structure containing sub-structures is simply a composite structure.

#### 4.1.2 Occurrence Declarations

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

Occurrences transform structures: consume input structures and produce output structures. They compose sequentially via `connect` on occurrence ports. `in`/`out` on ports express flow direction.

#### 4.1.3 Constraint Declarations

```
constraint def MinWallThickness {
    param wall : Length
    param process : ManufacturingProcess

    wall >= process.min_wall_thickness
}

constraint def Coaxial {
    param a : CylindricalFeature
    param b : CylindricalFeature

    distance(a.axis, b.axis) == 0mm
    angle(a.axis.direction, b.axis.direction) == 0deg
}
```

Bare expressions in a constraint body are assertions (predicate lines). Default connective between predicate lines is `and` (conjunction). Constraints are first-class entities: named, parameterized, composed, inherited, collected into libraries.

#### 4.1.4 Field Declarations

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

field def composite_stiffness : Point3<Length> -> Tensor<2, 3, Pressure> {
    source = composed {
        |p| if region_a.contains(p) then stiffness_a(p)
            else if region_b.contains(p) then stiffness_b(p)
            else base_stiffness
    }
}
```

Fields have a domain -> codomain type signature and a source.

| Source kind  | Meaning |
|-------------|---------|
| `analytical` | Closed-form expression, given as a lambda |
| `sampled`    | Discrete samples on a grid/mesh, with interpolation |
| `composed`   | Combination of other fields via arithmetic, logic, or conditional |
| `imported`   | External data file (OpenVDB, CSV, HDF5, etc.) |

### 4.2 Trait Declarations

Traits are non-entity declarations: no identity, no determinacy state, no evaluation graph presence. They are named, composable bundles of requirements.

```
trait_decl ::= 'pub'? 'trait' TYPE_IDENT type_params? (':' trait_ref ('+' trait_ref)*)? where_clause? '{' trait_member* '}'
```

Example:

```
pub trait Rigid : Physical {
    let moment_of_inertia = compute_moi(geometry, material.density)
}
```

**What a trait contains:**

| Member kind       | Description |
|-------------------|-------------|
| Parameters        | Required named parameters with types |
| Ports             | Required interaction points |
| Sub-structure slots | Required contained sub-structures satisfying a trait |
| Associated types  | Type-level members that implementing types must bind |
| Constraints       | Logical requirements on relationships between members |
| `let` bindings    | Values computed from other members -- both a requirement and a default definition, overridable |

**What traits do NOT contain:**
- Geometry -- traits can require geometric parameters and constrain geometry, but geometric bodies belong to implementing structures.
- Identity or state -- traits are stateless bundles of requirements, never directly instantiated.
- Implementation logic -- no procedural code, no method bodies. `let` bindings have declarative expressions (formulas), not procedures.

**Trait composition -- conflict resolution:**
- Same name, same type -> merge silently. A single member satisfies both trait requirements.
- Same name, different type -> error. v0.1 requires exact type match, not subtype compatibility.
- Constraint composition -> conjunction. All constraints from all composed traits must hold simultaneously.

**Trait refinement:** Additive requirements only. Can tighten parent constraints, narrow associated types, override defaults. Multiple refinement: `trait MechatronicActuator : MechanicalActuator + ElectricalDevice + Controllable`.

**Default values in traits:**

```
trait StandardThread {
    param handedness : Handedness = Handedness.Right
}

trait Cylindrical {
    param diameter : Length
    param length : Length
    let volume = pi * (diameter/2)^2 * length
}
```

**Conformance checking:** Nominal + structural hybrid. Explicit trait declaration (`: BoltShaped`) is the primary mode. Structural conformance is available as a query/analysis tool but doesn't change the type.

**Conformance interleaving with determinacy:** Conformance is interleaved with determinacy. A fully `undef` structure trivially conforms (constraints vacuously satisfiable). Constraints are checked as parameters become determined. Full conformance is only verifiable when all relevant parameters are determined.

### 4.2.1 Overloading Rules

**Entity definition overloading is not supported.** Two entity definitions (structures, occurrences, constraints, fields) with the same name but different parameter types are an error. Overloading entity definitions interacts badly with the determinacy spectrum -- when a parameter is `auto` or partially constrained, the compiler may not know which definition to select. Use traits instead.

**Function (`fn`) overloading by parameter types is permitted.** Functions are pure computations with no determinacy state. At every call site, argument types are statically known (even when values are `undef` or `auto`), so dispatch is unambiguous. Resolution rule: the compiler matches argument types against all candidates with the same name; exactly one candidate must match. If zero or more than one match, the call is an error. Implicit `Int` -> `Real` promotion is NOT considered during overload resolution.

```
// Valid: same name, different parameter types
fn area(surface: Surface) -> Scalar<Area> { ... }
fn area(solid: Solid) -> Scalar<Area> { ... }

// Valid: same name, different arity
fn rotate<G: Transformable>(geometry: G, axis: Vector3<Dimensionless>, angle: Angle) -> G { ... }
fn rotate<G: Transformable>(geometry: G, orientation: Orientation<3>) -> G { ... }
```

### 4.3 Function Declarations (`fn`)

Functions are non-entity declarations: no identity, no determinacy state, no evaluation graph presence. They are pure computations.

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

Semantics:
- **Pure** -- no side effects, no state.
- **Block body** -- `{ }` block containing `let` bindings and a final expression (the return value). No `return` keyword.
- **Type annotations mandatory** on parameters and return type.
- **Can be `pub`** for cross-module reuse.
- **Supports type parameters:** `fn distance<Q: Dimension>(a : Point3<Q>, b : Point3<Q>) -> Scalar<Q> { ... }`
- **`@optimized` hook available** for built-in fast paths.
- **Recursion permitted.** Self-recursion and mutual recursion are both permitted. Infinite recursion is a runtime error (stack overflow), not a compile-time error. The compiler does not attempt termination checking.

`fn` body's `{ }` block is a lexical scope -- `let` bindings inside are local. Not an entity scope -- no `self`, no determinacy tracking.

### 4.4 Purpose Declarations

Purposes are named, parameterized declaration kinds with AST identity. Semantically equivalent to a scope containing zero or more `constraint` declarations and/or `Output` occurrence instantiations. They have activation/deactivation mechanics via implementation-defined UX.

```
purpose_decl ::= 'pub'? 'purpose' IDENT type_params? '(' purpose_params ')' '{' purpose_member* '}'
```

Example:

```
purpose manufacturing_ready(subject : Structure) {
    constraint forall p in subject.geometric_params: determined(p)
    constraint forall p in subject.material_params: determined(p)
    minimize subject.cost
}
```

When activated, a purpose's constraints and outputs are present in the evaluation graph; when deactivated, they are absent. The checking/solving/proposing mode is determined by input determinacy state, not explicit mode selection.

**Purpose parameters are entity references, not values.** Purpose parameters bind to entities in the evaluation graph, not to values on the determinacy spectrum. The type annotation is an entity-kind selector: `Structure`, `Occurrence`, `Constraint`, or `Field`. These are built-in keywords in purpose parameter position, not value types -- `param x : Structure` inside a structure body is not valid.

Entity references provide **compiler-generated reflective access** over the bound entity's schema:

| Reflective member | Returns | Meaning |
|-------------------|---------|---------|
| `.params` | `List<ParamRef>` | All `param` declarations on the entity |
| `.geometric_params` | `List<ParamRef>` | Parameters whose type has nonzero geometric dimension exponents (Length, Area, Angle, etc.) |
| `.material_params` | `List<ParamRef>` | Parameters whose type is a material trait or contains material properties |
| `.sub_entities` | `List<EntityRef>` | All `sub` declarations |
| `.ports` | `List<PortRef>` | All `port` declarations |
| `.constraints` | `List<ConstraintRef>` | All constraint declarations |

These are schema queries resolved at elaboration time. Named members on the bound entity (e.g., `subject.cost`) resolve to the entity's ValueCells through the entity binding, and participate in the evaluation graph normally.

Purposes are graph-level constructs. They express requirements and objectives over the *shape and state* of a design, not computations over values. This is what makes them universally applicable -- `manufacturing_ready` works on any structure without per-structure opt-in.

### 4.5 Enum Declarations

v0.1 enums are C-style (no associated data):

```
enum Directionality { In, Out, Bidi }
enum FitType { Clearance, Transition, Interference }
enum ThreadSystem { ISO_Metric, ISO_Metric_Fine, UNC, UNF }
```

### 4.6 Unit Declarations

Units are declared with the `unit` keyword, populating the unit namespace:

```
unit mm : Length = 0.001m
unit USD : Money
unit degC : Temperature offset 273.15K
```

The compiler reads unit declarations from `std.units` at a bootstrap stage before parsing user code.

### 4.7 Member Declarations

#### `param` -- Value Parameters

```
param thickness : Length
param width : Length = 50mm
param material : M
param coating : Option<CoatingSpec> = none
```

Parameters are the public interface of a structure. Type annotation is mandatory. Default value optional (if absent, parameter is `undef`). Three-way distinction for unspecified parameters:
1. No default, not specified -> `undef`
2. Has default, not specified -> default value
3. Explicitly `undef` -> `undef` even if default exists

#### `port` -- Interaction Points

```
port mount_face : MechanicalPort { direction = in }
port shaft : RotaryPort
port workpiece_a : in StructurePort
```

Ports are typed scopes with members, uniform across structure ports and occurrence ports. Access via dot notation.

**Port type requirement:** A port's type annotation must be a trait that refines `Port` (defined in `std.ports`, re-exported in the prelude). The `Port` trait provides the `direction` parameter; domain-specific port traits (`MechanicalPort`, `RotaryPort`, etc.) refine it with additional interface parameters. A declaration `port x : T` where `T` does not refine `Port` is a compile error.

- **Structure ports** contain interface parameters defined by the port's trait.
- **Occurrence ports** contain a primary payload plus port-level parameters.

**Implicit deref for single-payload ports:** If a port type has exactly one member of a "transportable" type, that member is the port's primary payload. In expression contexts where the expected type matches, the port reference implicitly resolves to the payload. In `connect` statements, port references always mean the port itself.

#### `sub` -- Contained Sub-entities

```
sub motor : ElectricMotor { shaft_diameter = 8mm }
sub vents : List<Vent>
sub rib : Rib { height = thickness * 0.8 }

// Placement: `at` pose clause (v0.6)
sub bolt : Bolt at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
sub gear : Gear { teeth = 24 } at mount_frame

// Construction placement: `aux` modifier (v0.6)
aux sub jig : Jig at tool_frame
```

`sub` is the keyword for instantiating a contained sub-entity within a parent body. Sub-entities are named children in the containment tree.

**Instantiation syntax:** `sub name : Type { param = value, ... }`. Curly-brace block optional if no parameters overridden.

**`at` placement clause (v0.6).** An optional trailing `at <pose>` expression specifies the child's placement relative to the parent frame. The pose expression must evaluate to a `Transform` (child-frame → parent-frame) or a `Frame` (lowered to the equivalent `Transform`). The pose is an ordinary expression: `transform3(…)`, `frame3(…)`, a `let`-bound or port-supplied frame, or a bare identifier. When `at` is absent the child is authored directly in the parent frame (identity placement).

**`aux` modifier (v0.6).** Prefixing `aux` marks the sub-entity as structure-local (construction) geometry. An `aux sub` is still realized, tessellated, and shipped to the GUI (hidden-by-default, toggleable) but is excluded from product surfacing, STEP export, FEA mesh generation, and mass-property accumulation. Use `aux` to mark boolean-input operands so they do not appear both standalone and inside a composed result (see §8.3 for the boolean-composition idiom and §15 for the grammar production).

#### `let` -- Computed Bindings

```
let volume = thickness * width * height                    // Type inferred
let mass : Mass = volume * material.density                // Type annotated
pub let torque_constant : Torque/Current = back_emf / rated_speed
```

Named, typed, computed value with mandatory initialiser expression. Cannot be set from outside -- not a configurable parameter. Private by default; `pub let` to expose. Type annotation optional (type always inferrable from expression via dimensional analysis).

`aux let` marks construction geometry (intermediate solids, reference sketches) that is realized and shipped to the GUI but excluded from product surfacing, export, and analysis; see §8.3 (boolean-composition idiom) and §15 (grammar).

#### `type` -- Type Aliases

```
type Pressure = Force / Area
type StressTensor = Tensor<2, 3, Pressure>
type Point3<Q> = Point<3, Q>
```

Named type alias. The RHS is a type expression. Type aliases are transparent -- `Pressure` and `Force / Area` are the same type. Can be parameterized with type parameters. Can be `pub` for cross-module reuse.

#### `constraint` -- Inline Constraints

```
constraint thickness > 1mm
constraint head_diameter > shank_diameter
constraint forall f in faces: f.flatness < 0.01mm
```

Anonymous predicate that must hold. Can reference determinacy predicates:

```
constraint forall p in geometric_params: determined(p)
```

### 4.8 `meta` Blocks

```
structure def Bracket : Rigid {
    meta {
        description = "L-shaped mounting bracket for sensor array"
        part_number = "BRK-2024-001"
        revision = "C"
        compliance = "ISO 9001"
    }

    param thickness : Length
    param width : Length = 50mm
}
```

Semantics:
- Keys are identifiers (same lexical rules as other identifiers).
- Values are string literals.
- No types, no determinacy tracking, no constraint participation.
- Purely informational -- metadata is opaque to the evaluation graph.
- At most one `meta` block per entity body or module. Duplicates are a compile error.
- No duplicate keys within a `meta` block.
- Not inherited through traits or specialization.
- Accessible via `entity.meta.key_name`.

`meta` blocks can appear in any entity body and at the module level.

---

## 5. Expressions and Operators

### 5.1 Arithmetic Operators

```
a + b       a - b       a * b       a / b
a ^ n       -a          a % b       // modulo (integers only)
```

All arithmetic is dimensionally checked. `5mm + 3mm` = `8mm`. `5mm + 3kg` = type error. `5mm * 3mm` = `15mm^2`.

**Operator type rules:**

`Int` promotes to `Real` (= `Scalar<Dimensionless>`) when the other operand is a dimensioned type. When both operands are `Int`, no promotion occurs (except `/`). Operations not listed are type errors.

**Addition / subtraction (`+`, `-`):**

| Left | Op | Right | Result |
|------|----|-------|--------|
| `Int` | +, - | `Int` | `Int` |
| `Scalar<Q>` | +, - | `Scalar<Q>` | `Scalar<Q>` (same `Q` required) |
| `Vector<N,Q>` | +, - | `Vector<N,Q>` | `Vector<N,Q>` |
| `Point<N,Q>` | + | `Vector<N,Q>` | `Point<N,Q>` |
| `Vector<N,Q>` | + | `Point<N,Q>` | `Point<N,Q>` |
| `Point<N,Q>` | - | `Point<N,Q>` | `Vector<N,Q>` |
| `Matrix<M,N,Q>` | +, - | `Matrix<M,N,Q>` | `Matrix<M,N,Q>` |
| `Tensor<R,N,Q>` | +, - | `Tensor<R,N,Q>` | `Tensor<R,N,Q>` |

`Point + Point` is a type error. Temperature addition follows Section 3.2 rules.

**Multiplication (`*`):**

| Left | Right | Result |
|------|-------|--------|
| `Int` | `Int` | `Int` |
| `Scalar<Q1>` | `Scalar<Q2>` | `Scalar<Q1*Q2>` |
| `Scalar<Q>` | `Vector<N,Q2>` | `Vector<N, Q*Q2>` |
| `Scalar<Q>` | `Matrix<M,N,Q2>` | `Matrix<M,N, Q*Q2>` |
| `Scalar<Q>` | `Tensor<R,N,Q2>` | `Tensor<R,N, Q*Q2>` |
| `Matrix<M,K,Q1>` | `Matrix<K,N,Q2>` | `Matrix<M,N, Q1*Q2>` |
| `Matrix<M,K,Q1>` | `Vector<K,Q2>` | `Vector<M, Q1*Q2>` |

Scalar-multiply is commutative (right-operand forms omitted). `Q1*Q2` means element-wise addition of dimension exponent vectors. `Vector * Vector` is not defined -- use `dot()` or `cross()`.

**Division (`/`):**

| Left | Right | Result |
|------|-------|--------|
| `Int` | `Int` | `Real` |
| `Scalar<Q1>` | `Scalar<Q2>` | `Scalar<Q1/Q2>` |
| `Vector<N,Q>` | `Scalar<Q2>` | `Vector<N, Q/Q2>` |
| `Matrix<M,N,Q>` | `Scalar<Q2>` | `Matrix<M,N, Q/Q2>` |
| `Tensor<R,N,Q>` | `Scalar<Q2>` | `Tensor<R,N, Q/Q2>` |

`Int / Int` produces `Real` -- no silent truncation. Use `floor(a / b)` for truncating integer division. Division by a non-scalar is not defined; use `inverse()` for matrices.

**Exponentiation (`^`):**

| Base | Exponent | Result |
|------|----------|--------|
| `Int` | `Int` (>= 0) | `Int` |
| `Real` | `Real` | `Real` |
| `Scalar<Q>` | integer literal `n` | `Scalar<Q^n>` |

`Scalar<Q> ^ n` scales the dimension exponent vector by `n`. Non-integer exponents on dimensioned quantities are type errors in v0.1 (use `sqrt` for half-integer exponents).

**Unary negation (`-`) and modulo (`%`):**

Negation is defined for `Int`, `Scalar<Q>`, `Vector<N,Q>`, `Matrix<M,N,Q>`, `Tensor<R,N,Q>` -- returns the same type. Modulo is `Int % Int -> Int` only.

### 5.2 Comparison Operators

```
a == b      a != b
a < b       a > b       a <= b      a >= b
```

Equality is structural for value types, identity for structure references.

**Chained comparisons:** `a < b < c` desugars to `a < b and b < c`. Any comparison operators may chain.

```
2mm < thickness < 10mm       // desugars to: 2mm < thickness and thickness < 10mm
0 < poissons_ratio < 0.5
```

**Geometric equality:** Two distinct operations:
- **Identity equality (`==` on geometry):** Compares specification identity (same evaluation graph node). Cheap, exact, deterministic.
- **Geometric equivalence (`geo_equiv`):** `fn geo_equiv(a: Geometry, b: Geometry, tolerance: Length) -> Bool`. Expensive, approximate, explicitly requested.

**Comparison type rules:**

| Type | `==` `!=` | `<` `>` `<=` `>=` |
|------|-----------|---------------------|
| `Bool` | yes | no |
| `Int` | yes | yes |
| `Scalar<Q>` | yes (same `Q`) | yes (same `Q`) |
| `String` | yes | no |
| `Vector<N,Q>`, `Point<N,Q>` | yes (same `N`, `Q`) | no |
| `Matrix`, `Tensor` | yes (same shape, `Q`) | no |
| `Option<T>` | yes (if `T` supports `==`) | no |
| Enum variants | yes (same enum) | no |
| Geometry types | identity only (see above) | no |

All comparisons return `Bool`. `Int` promotes to `Real` when compared with `Scalar<Q>`. Ordering (`<`, `>`, `<=`, `>=`) is defined only for scalar numeric types.

### 5.3 Logical Operators

```
a and b     a or b      not a
a implies b
```

Keywords, not symbols. `and` and `or` are used instead of `&&` and `||`.

**Logical type rules:** All logical operators (`and`, `or`, `not`, `implies`) take `Bool` operands and return `Bool`. No truthy/falsy coercion -- `0`, `none`, empty collections, and `""` are not implicitly boolean. When operands are `undef`, logical operators follow Kleene three-valued logic (see Section 9.2.3).

### 5.4 Quantifiers (`forall`, `exists`)

Generalized to both expressions and statements:

```
// Expression form (produces Bool)
forall v in vents: v.spacing > 10mm
exists b in bolts: b.grade >= 10.9

// Statement form (per-element connect or constraint)
forall v in vents: connect v.inlet -> housing.air_channel
forall v in vents: constraint v.mass < 50g
```

Disambiguation is by the token after the colon: `connect` or `constraint` for statements, otherwise expression.

v0.1 statement scope: `connect` and `constraint` only.

**Empty collections:** `forall` over an empty collection is `true` (vacuous truth). `exists` over an empty collection is `false` (vacuous falsity). See Section 3.4.

**`undef` propagation:** Quantifiers follow Kleene semantics (iterated `and` / `or`). See Section 9.2.6 for the full truth table.

**Guarded collections:** A quantifier over a `where`-guarded collection is subject to reference safety (Section 8.10) -- the quantifier must be under the same or stronger guard. When the guard is active, the quantifier operates on the collection's current contents. When the guard is inactive, the quantifier is absent from the evaluation graph entirely (not vacuously true -- simply not present).

### 5.5 Conditional Expressions

```
if condition then expr_a else expr_b
```

Expression-level conditional (always has both branches, always produces a value). Not a statement-level `if`. When the condition is determined, only the selected branch contributes; the unselected branch's determinacy is irrelevant. When the condition is `undef`, the result is `undef`. See Section 9.2.4.

### 5.6 Lambda Expressions

```
|x| x * 2
|p : Point3<Length>| distance(p, origin)
|a, b| a.thickness + b.thickness
```

Parameter types inferred where possible, annotatable where needed. Primary use: analytical field definitions, inline predicates, higher-order constraint helpers. Follows Rust syntax convention.

### 5.7 Member Access

```
bracket.thickness               // Parameter access
bracket.mount_face              // Port access
bracket.mount_face.frame        // Nested access
bracket.rib.height              // Sub-structure member access
bracket.meta.part_number        // Metadata access
```

Dot-notation. Chained access resolves through the containment tree. Unlimited dot-chain depth permitted; compiler warns on deep chains (threshold configurable, suggested default: 3-4 levels).

### 5.8 Qualified Trait Access

```
Fastener::rated_load                      // Disambiguate when two traits define same name
bracket.(Rigid::max_temperature)          // Instance-level qualified access
```

### 5.9 Collection Expressions

```
[1, 2, 3]                       // List literal
set{a, b, c}                    // Set literal
map{"key" => value, "k2" => v2} // Map literal

list.map(|x| x * 2)             // Map over collection
list.filter(|x| x > 5mm)        // Filter
list.fold(0mm, |acc, x| acc + x)  // Fold/reduce
list.all(|x| x > 0mm)           // Universal quantifier (returns Bool)
list.any(|x| x > 100mm)         // Existential quantifier (returns Bool)
list.count                       // Size
list.sum                         // Sum (for numeric collections)
```

**`List.generate` combinator:**

```
let bolt_positions = List.generate(bolt_count, |i|
    point3(radius * cos(i * 2 * pi / bolt_count), radius * sin(i * 2 * pi / bolt_count), 0mm)
)
```

### 5.10 `match` Expressions

```
let drive_size = match head_type {
    Hex => across_flats * 0.9
    Socket => socket_diameter
    Button => socket_diameter
    Flat => none
}

let total = match coating {
    some(c) => base + c.thickness
    none => base
}
```

Exhaustiveness enforced. Wildcard `_` catches remaining cases. Multiple variants with `|`: `Socket | Button => recessed_drive`. No fall-through. When the discriminant is `undef`, the result is `undef` (see Section 9.2.5).

### 5.11 Indexing

```
vents[0]            // List indexing
params["key"]       // Map lookup
```

Grammar: `expr ::= ... | expr '[' expr ']'`

### 5.12 `undef` and `auto` in Expressions

```
param thickness : Length = undef     // Explicit undef (overrides default)
param width : Length = auto          // Delegated to solver

let area = thickness * width         // area is undef if either input is undef
```

`undef` and `auto` are valid in any expression position where a value is expected. Detailed propagation rules for `undef` through all operator and expression types are specified in Section 9.2.

---

## 6. Statements

### 6.1 `connect`

```
connect motor.shaft -> coupling.driver
connect coupling.driven -> gearbox.input : SplineConnection { tooth_count = 24 }
connect plate_a.face <-> plate_b.face : ButtWeld
```

`connect` is a statement-level construct. `->` indicates connection direction. `<->` for explicitly bidirectional connections.

**Semantic decomposition.** A `connect` statement is not a primitive -- it desugars into concrete artifacts that participate in the evaluation graph:

1. **Connector structure instance** -- if a connector type is specified (e.g., `: ShrinkFit`), an instance of that type is created as a sub-structure. If no connector type is given, only constraints are generated (no connector instance). The connector instance is a normal structure with parameters, ports, and constraints; it can be referenced by name for inspection or further constraining.
2. **Port compatibility constraints** -- type-checked assertions that the connected ports are compatible (matching or complementary traits, `In` <-> `Out` directionality).
3. **Connector-port binding constraints** -- constraints from the connector type's trait definition that relate connector parameters to port parameters on both sides.
4. **Frame alignment constraints** -- when both ports are geometrically located (`LocatedPort`), the compiler generates constraints that align the port frames according to the connector type's alignment semantics (e.g., coincident origins, matching orientations). When no connector type is given, default frame coincidence is assumed.
5. **Topology edge** -- a directed (or bidirectional) edge in the assembly topology graph, used for traversal, visualization, and connectivity queries.

All generated artifacts are accessible via the containing scope. The connector instance (if any) can be referenced by an auto-generated name derived from the connected ports, or explicitly named via `let`:

```
connect motor.shaft -> coupling.driver : SplineConnection { tooth_count = 24 }
// The connector instance is accessible as motor_shaft__coupling_driver (auto-named)
// or explicitly: let shaft_spline = connect motor.shaft -> coupling.driver : SplineConnection { ... }
```

**Connector ownership:** The connector structure instance is owned by the nearest common ancestor of the connected ports by default. A designer override is available to place it elsewhere.

**Cyclic connection topology:** Cyclic connection graphs are supported (e.g., four-bar linkages, closed kinematic chains).

**Concision trade-off:** A single `connect` statement can generate multiple structures and constraints that are not visible in the source text. This favors concision over explicitness. The tooling must make generated artifacts inspectable -- a "connection inspector" showing all generated constraints and the connector instance is expected in the IDE.

#### 6.1.1 Connector Parameterization

```
connect housing.bore -> shaft.journal : ShrinkFit {
    interference = 0.02mm
    assembly_temperature_delta = 150degC
}

connect flange_a.face -> flange_b.face : M8Bolt {
    grade = 10.9
    count = 6
    pattern = BoltCircle { diameter = 80mm }
}
```

#### 6.1.2 Port Mapping

**Same-interface automatic matching:** When both sides implement the same interface trait, ports are matched by name:

```
connect motor.nema17 -> mount_plate.nema17 : NEMA17BoltSet { grade = 8.8 }
```

**Explicit mapping for different interfaces (all-or-nothing):**

```
connect motor.nema17 -> adapter.side_a {
    shaft -> input_bore
    bolt_hole_1 -> mounting_a
    bolt_hole_2 -> mounting_b
}
```

Every port on both sides must appear. Partial mappings not permitted. If a mapping block is provided where automatic matching would otherwise apply, the mapping block takes precedence.

**Mixing port mappings and connector parameters:**

```
connect motor.nema17 -> adapter.side_a : AdapterPlate {
    thickness = 5mm          // Connector parameter (= operator)
    shaft -> input_bore      // Port mapping (-> operator)
    bolt_hole_1 -> mounting_a
    bolt_hole_2 -> mounting_b
}
```

#### 6.1.3 Ad-hoc Connections

```
connect phone_case -> nut                                       // Minimal
connect phone_case -> nut : Adhesive { type = JBWeld }          // With connector
connect bracket@face(top_surface) -> plate@face(bottom_surface) : Adhesive  // Ad-hoc geometry
connect pipe@region(outer_surface, z = 0mm..50mm) -> clamp@region(inner_surface)
```

The `@` operator creates an ad-hoc port on a structure by designating a geometric region. Syntax: `structure@selector(arguments)`.

Standard library selectors:

| Selector      | Meaning |
|--------------|---------|
| `@face(name_or_expr)` | A named or computed surface |
| `@region(surface, constraints...)` | A sub-region of a surface |
| `@point(coordinates)` | A specific point |
| `@edge(name_or_expr)` | A named or computed edge |
| `@body(name_or_expr)` | A named or computed volume region |

**Geometry selector stability (v0.1 limitation).** Geometry selectors depend on persistent naming -- the ability to identify a geometric feature (face, edge, etc.) stably across parameter changes and geometry regeneration. Persistent naming is a known hard problem in parametric CAD.

v0.1 strategy:
- **Named features:** Selectors referencing features by construction-history name (e.g., `@face(top)` where `top` was explicitly named during geometry construction) are stable.
- **Computed features:** Selectors using geometric queries (e.g., `@face(faces_by_normal(solid, vec3(0, 0, 1), 1deg)[0])`) may become invalid when upstream parameters change the topology (e.g., a fillet removes an edge, a boolean changes face count).
- **Failure behavior:** When a selector cannot resolve to a geometric feature, the ad-hoc port's frame becomes `undef`, and any constraints referencing it propagate `undef` or become `indeterminate`. A diagnostic is emitted identifying the broken selector and the parameter change that caused the failure.

Strengthened persistent naming and advanced topological queries are deferred to v0.2 (see Section 18).

### 6.2 `chain`

```
chain casting -> machining -> heat_treat -> finishing
```

`chain` is sugar for connecting each occurrence's default output port to the next's default input port. Uses fully implicit matching via default ports only. Non-default port mapping requires explicit `connect`.

**Desugaring:** `chain` is expanded to a sequence of `connect` statements before evaluation graph construction. Each element must be an occurrence with exactly one `out` port and one `in` port (or ports marked as default for their direction). The desugaring is:

```
chain casting -> machining -> heat_treat -> finishing
// Desugars to:
connect casting.default_out -> machining.default_in
connect machining.default_out -> heat_treat.default_in
connect heat_treat.default_out -> finishing.default_in
```

If any element has multiple `in` or `out` ports and none is marked as default, `chain` is a compile error for that element. The designer must use explicit `connect` statements instead.

### 6.3 `where` Guards and Blocks

**`where` has uniform semantics across all entity types:** it controls structural presence. When false, the entity does not exist in the evaluation graph.

**Per-declaration guard (post-name for bodies, post-expression for bodyless):**

```
sub fan_mount : FanMount where needs_cooling { ... }
constraint vent_count >= 2 where needs_cooling
```

Rule: `where` comes after the "what" and before the body (if any).

**Block-level `where`:**

```
where needs_cooling {
    constraint vent_count >= 2
    sub fan_mount : FanMount { ... }
    sub vents : List<Vent> { ... }
}
```

Desugars to per-declaration guards. `where` blocks do NOT introduce a new lexical scope.

**`else` clause (v0.1):**

```
where needs_cooling {
    sub fan_mount : FanMount { ... }
} else {
    sub passive_vents : List<PassiveVent> { ... }
}
```

Members inside the `else` block desugar to individual `where !condition` guards.

**Nesting:** `where` blocks nest, with guards composing conjunctively:

```
where needs_cooling {
    sub fan_mount : FanMount { ... }
    where high_airflow {
        sub secondary_fan : Fan { ... }
        // Present only when needs_cooling AND high_airflow
    }
}
```

**Mixing per-declaration and block guards:** Compose conjunctively:

```
where needs_cooling {
    sub fan_mount : FanMount { ... }
    sub backup_fan : Fan where redundancy_required { ... }
    // backup_fan present when needs_cooling AND redundancy_required
}
```

**Reference safety rule:** Referencing a guarded entity from an unguarded context is a compile error. A reference is valid only if the referencing declaration's guard implies the referenced entity's guard.

```
where needs_cooling {
    sub fan_mount : FanMount { size = 40mm }
}

// Compile error: fan_mount guarded by needs_cooling, case_width is not
let case_width = fan_mount.width + 2 * wall_t

// Valid: same guard covers the reference
let case_width = fan_mount.width + 2 * wall_t where needs_cooling
```

### 6.4 `match` Blocks (Declaration-Level)

```
match head_type {
    Hex => sub head : HexHead { ... }
    Socket => sub head : SocketHead { ... }
    Button => sub head : ButtonHead { ... }
    Flat => sub head : FlatHead { ... }
}
```

Desugars to `where` guards with exhaustiveness checking:

```
sub head : HexHead where head_type == HeadType.Hex { ... }
sub head : SocketHead where head_type == HeadType.Socket { ... }
// ...
```

**Same-name guarded declarations:** The desugaring produces multiple declarations with the same name (`head`) but different types. This is permitted when the guards are mutually exclusive (which exhaustive `match` guarantees). The declarations are treated as a single logical entity -- external references use the shared name (`self.head`), and the type at any reference site is the union of the possible types, narrowed by the active guard. This exception to the normal "one name per scope" rule applies only to declarations with provably mutually exclusive guards.

Multiple variants with `|`: `Socket | Button => sub head : RecessedHead { ... }`.

---

## 7. Module System

### 7.1 File/Module Mapping

- Every `.ri` file is exactly one module. A file cannot contain multiple module declarations, and a module cannot span multiple files.
- Every `.ri` file must begin with a `module` declaration specifying its full path.
- The declared path must match the file's location in the source tree (enforced by tooling).

```
module std.mechanical.fasteners.bolt
// This file must be located at std/mechanical/fasteners/bolt.ri
```

- Directories are namespaces, not modules. A directory may contain a `mod.ri` file serving as the directory-level module, primarily for curating the public API via re-exports.

```
// std/mechanical/fasteners/mod.ri
module std.mechanical.fasteners

pub import std.mechanical.fasteners.bolt.Bolt
pub import std.mechanical.fasteners.nut.Nut
pub import std.mechanical.fasteners.washer.Washer
```

### 7.2 Module Declaration

```
module company.products.actuators
```

One per file, at the top. Module path corresponds to file location.

### 7.3 Import Forms

| Form | Syntax | Effect |
|------|--------|--------|
| Module import | `import std.mechanical.fasteners` | Qualified access via module name |
| Entity import | `import std.mechanical.fasteners.Bolt` | Unqualified access to `Bolt` |
| Destructured import | `import std.mechanical.fasteners.{Bolt, Nut}` | Multiple entities unqualified |
| Module alias | `import std.mechanical.fasteners as f` | Alias for qualified access |
| Entity rename | `import std.mechanical.fasteners.Bolt as StdBolt` | Renamed unqualified access |

**What is importable:** Anything marked `pub` at the top level of a module: structure definitions, occurrence definitions, constraint definitions, field definitions, trait definitions, and `pub let` bindings.

**Full qualified paths always work:** A fully qualified path resolves regardless of whether the module has been imported. Imports introduce shorter names -- they do not gate access.

```
// No import needed:
sub my_bolt : std.mechanical.fasteners.Bolt { ... }
```

**Aliases are additive:** An alias introduces an additional name; it does not replace the original.

**No wildcard imports:** `import std.mechanical.fasteners.*` is not supported. Single exception: the prelude.

### 7.4 Visibility (`pub`)

```
pub structure def Bracket { ... }           // Public -- visible outside module
structure def InternalHelper { ... }        // Module-private (default)
```

Default visibility is private. `pub` applies to definitions and to members within definitions:

```
pub structure def Motor : Rigid {
    param rated_torque : Torque                // Visible (param default)
    port shaft : RotaryPort                    // Visible (named occurrence default)
    let winding_resistance = ...               // Private (local default)
    pub let torque_constant = ...              // Visible (pub override)
}
```

Member-level visibility rules:
- **Parameters** and **named sub-entities** (including ports) are visible from outside.
- **`let` bindings** and **constraints** are private by default.
- `pub let` exposes a computed value without promoting it to a parameter.
- No `priv` modifier in v0.1.

### 7.5 Re-exports

```
pub import internal.helper.UsefulTrait
```

Re-exports are transparent -- the entity appears as if defined in the re-exporting module.

### 7.6 Prelude

Every module implicitly imports `std.prelude`. The user never writes this import.

**Prelude contents (v0.1):**

- **Primitive types:** `Bool`, `Int`, `Real`, `String`
- **Physical quantity dimension aliases:** All 35 named dimensions (including `Dimensionless`)
- **Unit literals:** All SI units with prefixes, core imperial units, `pi`
- **Core math:** `abs`, `min`, `max`, `clamp`, `sqrt`, `lerp`, `remap`, `pow`, `log`, `log10`, `exp`, `sign`, `floor`, `ceil`, `round`, `mod`
- **Trigonometry:** `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `sinh`, `cosh`, `tanh`
- **Linear algebra:** `dot`, `cross`, `normalize`, `magnitude`
- **Geometry constructors:** `point2`, `point3`, `vec2`, `vec3`, `orient_axis_angle`, `orient_quaternion`, `orient_euler`, `orient_basis`, `orient_look_at`, `orient_identity`, `frame3`, `frame3_identity`, `transform3`, `transform3_identity`, `project`
- **Ports:** `Port`, `Directionality`
- **Determinacy predicates:** `determined()`, `constrained()`, `undetermined()`, `partially_determined()`

**Suppression:** The pragma `#no_prelude` suppresses the implicit prelude import.

### 7.7 No Circular Dependencies

The module dependency graph must be a directed acyclic graph (DAG). If module A imports module B (directly or transitively), module B cannot import module A. If two definitions genuinely need mutual references, they belong in the same module.

---

## 8. Name Resolution and Scoping

### 8.1 Lexical Scoping

All name resolution is lexical. A name reference resolves to the lexically enclosing declaration. There is no dynamic scoping.

### 8.2 Order-Independence

All declarations within a body are mutually visible regardless of textual order. A constraint may reference a parameter declared below it. The language is declarative. Import statements are conventionally placed at the top but need not precede the declarations that use them.

### 8.3 Downward Visibility

A parent scope accesses a child's declarations via dot notation: `motor.shaft_diameter`. Dot chains compose: `assembly.bracket.hole.diameter`.

**Visibility boundary:** Only **parameters** and **named sub-entities** (ports, sub-structures) are accessible from outside a scope. **`let` bindings** and **constraints** are private (unless marked `pub`).

**Auto-surfacing (v0.6 — recommended idiom).** Any `sub` that carries an `at`
pose clause — or any plain containment sub whose child declares a geometry body
— auto-surfaces at its composed world pose.  The parent need not re-express
descendant bodies: the surfacing walk composes transforms up the containment
tree and emits each child's mesh at its computed world position automatically.
Use `at` for placement and let auto-surfacing do the rest:

```reify
pub structure Bolt { let body = cylinder(5mm, 20mm) }
pub structure Gear { let body = cylinder(40mm, 15mm) }

pub structure Assembly {
    sub bolt : Bolt at transform3(orient_identity(), vec3(30mm, 0mm, 0mm))
    sub gear : Gear { teeth = 24 } at mount_frame
    // No manual lift needed — bolt and gear surface at their composed world poses.
}
```

**Cross-sub geometry composition (boolean idiom).** Geometry-typed members on
a non-collection sub — whether declared as `param body : Solid = box(...)` or
as `let body = box(...)` on the child — are accessible from the parent via
`self.<sub>.<member>` dot notation. The access lowers to a stable reference to
the child's realization handle, so transforms and boolean ops in the parent
compose directly over the child's body.  Use this idiom when a child body must
be cut into or unioned with a parent solid:

```reify
pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let placed = translate(self.inner.body, 10mm, 0mm, 0mm)
}

pub structure A { let body = box(10mm, 10mm, 10mm) }
pub structure B { let body = cylinder(5mm, 10mm) }
pub structure C {
    sub a = A()
    sub b = B()
    let combined = union(self.a.body, self.b.body)
}
```

**Avoiding double-surfacing.** When a child body is used as a boolean operand
(cut/union into a parent solid), mark the sub `aux` so the child's geometry
does not appear both as a standalone surface and inside the composed result
(PRD §3 rule 3 / §11.5):

```reify
pub structure Housing {
    aux let blank = box(50mm, 30mm, 20mm)   // operand — not surfaced standalone
    aux let bore  = cylinder(8mm, 25mm)     // operand — not surfaced standalone
    let body = cut(self.blank, self.bore)   // only this surfaces
}
```

Implemented in task 3441 (cross-sub access); `at` placement and `aux` suppression in task 3903.

**v0.1 limitations — remaining scope gaps:**

1. **Collection-sub geometry is deferred.** Cross-sub geometry access on a
   collection sub (e.g. `bolts[0].body` or bare `self.bolts.body`) is not yet
   supported and continues to emit an actionable diagnostic. Per-element
   realization handles require per-instance realization, which is out of
   scope for v0.1.
2. **Nested cross-sub access is deferred.** Chains deeper than one level —
   e.g. `self.outer.inner.body` — are not yet supported.
3. **Parameter overrides do not propagate into the child's body.** A sub's
   body is realised once per build using the child structure's own parameter
   defaults; parent-side parameter overrides on the sub do not flow into the
   realised geometry. For per-instance variation, parameterise the child's
   primitives and pass scalar arguments downward:

```reify
pub structure Inner {
    param offset_x : Length = 0mm
    let body = translate(box(10mm, 20mm, 30mm), offset_x, 0mm, 0mm)
}
pub structure Outer {
    sub inner = Inner(offset_x: 10mm)
}
```

### 8.4 Upward Visibility

A lexical scope has access to the bound names in the parent lexical scope, transitively. Children see parents implicitly; parents see children explicitly (via dot notation).

**Asymmetry rule:** A child's declarations do not enter the parent namespace.

### 8.5 Shadowing

Warn, not forbid. When a declaration in a child scope uses the same name as a declaration visible from a parent scope, the compiler emits a warning. The shadowing is permitted -- the child's declaration takes precedence within the child scope.

### 8.6 `self`

The `self` keyword refers to the enclosing entity definition or specialization. `self.param_name` is equivalent to `param_name` for locally declared names. Required when the entity itself (rather than one of its members) is the referent.

`self` never refers to the module. The module is not an entity.

### 8.7 Specialization Scopes

When a sub-entity is instantiated within a parent body, its body is a specialization scope:
- Sees the parent scope (and transitively, all ancestor scopes)
- Can set parameters and add constraints on the instance
- Does not modify the underlying definition

**Permitted in a specialization body:**

| Member kind | Meaning |
|-------------|---------|
| Parameter assignments | `thickness = 3mm` -- set a value for an existing parameter |
| `constraint` | Add constraints on the instance's parameters |
| `let` bindings | Local computed values (scoped to the specialization) |
| `connect` | Connect the instance's ports |
| `where` guards | Conditionally include any of the above |

**Not permitted:** New `param`, `port`, or `sub` declarations. A specialization configures an existing definition; it does not extend its schema. To add members, define a new structure that inherits via trait or composition.

```
structure def Assembly {
    param clearance : Length
    sub motor : ElectricMotor {
        shaft_diameter = 8mm
    }
    sub coupling : ShaftCoupling {
        bore = motor.shaft_diameter
        constraint bore - motor.shaft_diameter >= clearance
    }
}
```

### 8.8 Trait Member Merging

When a structure implements a trait, the trait's declared members become part of the structure's body. They are accessed without qualification -- as if declared directly in the structure.

Conflict resolution:

| Case | Result |
|------|--------|
| Same name, same type | No conflict. Single declaration satisfies both. |
| Same name, different type | Error (exact type match required in v0.1). |
| Same name, same type, different constraints | Both constraints apply (conjunction). |

### 8.9 Recursive Structures

Recursive structure definitions are permitted. Structural unfolding is eager -- once the parameters controlling recursion depth are determined, the full instance tree is materialized.

```
structure def TreeBracket {
    param depth : Int

    sub left : TreeBracket where depth > 0 {
        depth = self.depth - 1
    }
    sub right : TreeBracket where depth > 0 {
        depth = self.depth - 1
    }
}
```

**Termination requirement:** A recursive structure definition must have a termination condition (`where` guard, `Option` type, or variant type base case). A recursive definition with no reachable termination condition is a static error.

**`undef` is NOT a valid termination mechanism.** `undef` means "not yet decided," not "structurally absent."

**Unfolding preconditions:** Recursion-controlling parameters must be determined before structural unfolding proceeds.

**Resolution order:** Resolution does not cross recursive instance boundaries. Each instance resolves its own `auto` parameters independently. The evaluation graph resolves in dependency order (typically leaves first).

**Candidate evaluation errors for `auto` resolution (three-category taxonomy):**

1. **Candidate violates a constraint:** Normal solver operation. The candidate is pruned silently.
2. **All candidates exhausted / no feasible region:** Resolution failure. Propagates as indeterminate with a diagnostic explaining which constraints could not be jointly satisfied.
3. **Candidate triggers structural or type error:** Propagates as an error. This is a definition error, not a bad candidate -- it indicates a problem in the design itself rather than an infeasible parameter value.

### 8.10 Guarded Declaration Reference Safety

Referencing a guarded entity from an unguarded context is a compile error. A reference is valid only if the referencing declaration's guard implies the referenced entity's guard. Static implication check on boolean guard expressions.

### 8.11 Imports and Scoping

Imported names enter the module's top-level namespace. They do not participate in upward visibility -- imported names are not lexical parents. They are simply available as names in the module scope.

---

## 9. Determinacy Model

### 9.1 The Determinacy Spectrum

Every parameter sits on a determinacy spectrum:

1. **`undef`** -- Nothing said. Default state. Epistemic honesty: "we don't know yet."
2. **Constrained but not determined** -- Constraints narrow domain but no single value (`wall_thickness > 2mm`).
3. **`auto`** -- Delegated to the system. A decision, not an absence. "I want this to have a value; figure it out for me."
4. **Determined** -- Specific value or fully resolved expression (`wall_thickness = 3mm`).

### 9.2 `undef` Semantics

`undef` means "not yet decided." It is the default state of every unassigned parameter without a default value. `undef` is always valid syntactically and semantically -- a design with `undef` parameters is a legitimate, partially-specified design.

**Core propagation principle:** `undef` propagates strictly through computations, except where the result is provably independent of the unknown operand under the semantics of the operator. The rules below are exhaustive for v0.1.

#### 9.2.1 Arithmetic Operators

`undef` propagates strictly through all arithmetic. No algebraic identities are exploited.

| Expression | Result | Rationale |
|------------|--------|-----------|
| `undef + 5mm` | `undef` | Result depends on unknown operand |
| `5mm - undef` | `undef` | Result depends on unknown operand |
| `0 * undef` | `undef` | Strict; `0 * x = 0` not exploited |
| `undef * undef` | `undef` | Even for the same parameter |
| `undef ^ 0` | `undef` | Strict; `x^0 = 1` not exploited |
| `-undef` | `undef` | Unary negation propagates |
| `undef % 5` | `undef` | Modulo propagates |

**Rationale for strict arithmetic:** Exploiting algebraic identities (like `0 * x = 0`) would require the compiler to reason about mathematical properties of operators, creating implementation complexity and surprising edge cases (e.g., `0 / 0`, overflow). Strict propagation is simpler, predictable, and sufficient -- expressions like `0 * param` with a literal zero are vanishingly rare in engineering designs.

#### 9.2.2 Comparison Operators

Comparisons with any `undef` operand produce `undef` (of type `Bool`).

| Expression | Result |
|------------|--------|
| `undef < 5mm` | `undef` |
| `5mm == undef` | `undef` |
| `undef == undef` | `undef` |
| `2mm < undef < 10mm` | `undef` (desugars to `2mm < undef and undef < 10mm`) |

An `undef`-valued `Bool` in a constraint context makes the constraint **indeterminate** -- neither satisfied nor violated.

#### 9.2.3 Logical Operators (Kleene Three-Valued Logic)

Logical operators follow **Kleene's strong three-valued logic**, where `undef` acts as "unknown." A logical operator absorbs `undef` only when the result is determined regardless of the unknown operand's value.

| `a` | `b` | `a and b` | `a or b` | `not a` | `a implies b` |
|---------|---------|-----------|----------|---------|---------------|
| `true`  | `true`  | `true`    | `true`   | `false` | `true`        |
| `true`  | `false` | `false`   | `true`   |         | `false`       |
| `true`  | `undef` | `undef`   | `true`   |         | `undef`       |
| `false` | `true`  | `false`   | `true`   | `true`  | `true`        |
| `false` | `false` | `false`   | `false`  |         | `true`        |
| `false` | `undef` | `false`   | `undef`  |         | `true`        |
| `undef` | `true`  | `undef`   | `true`   | `undef` | `true`        |
| `undef` | `false` | `false`   | `undef`  |         | `undef`       |
| `undef` | `undef` | `undef`   | `undef`  |         | `undef`       |

Logical operators are **commutative with respect to `undef`** -- operand order does not affect propagation. This is consistent with Reify's declarative semantics (not imperative short-circuit evaluation).

**Rationale for Kleene but not algebraic identities in arithmetic:** Boolean absorption is cheap to verify (two rules per operator, finite truth table). Arithmetic identity exploitation opens unbounded complexity (must reason about `0 * infinity`, dimensional edge cases, etc.). The asymmetry is deliberate.

#### 9.2.4 Conditional Expressions

```
if condition then expr_a else expr_b
```

| `condition` | Result |
|-------------|--------|
| `true`      | `expr_a` (evaluated; `expr_b` does not contribute) |
| `false`     | `expr_b` (evaluated; `expr_a` does not contribute) |
| `undef`     | `undef` (neither branch can be selected) |

When the condition is determined, only the selected branch's determinacy matters. `if true then 5mm else undef` evaluates to `5mm`. The compiler does not attempt to prove branch equivalence when the condition is `undef` -- even `if undef then 5mm else 5mm` evaluates to `undef`.

#### 9.2.5 `match` Expressions

| Discriminant | Result |
|--------------|--------|
| Determined   | Result of the matching branch |
| `undef`      | `undef` (no branch can be selected) |

#### 9.2.6 Collection Operations

**Collection structure vs element values:** A collection's structure (count, keys) is independent of its element values. When the collection structure is determined but elements contain `undef`, structural operations produce determined results.

| Expression | Collection state | Result |
|------------|-----------------|--------|
| `list.count` | Structure determined (3 elements) | `3` (even if elements are `undef`) |
| `list.count` | List itself `undef` | `undef` |
| `list.sum` | Structure determined | `undef` if any element is `undef` |
| `list[i]` | Structure determined, valid index | Element value (may be `undef`) |
| `list[i]` | List itself `undef` | `undef` |
| `map[key]` | Structure determined, key present | Value (may be `undef`) |
| `map[key]` | Structure determined, key absent | Evaluation failure (not `undef`) |
| `map[key]` | Map itself `undef` | `undef` |

**`forall` and `exists`:** Quantifiers follow Kleene semantics, consistent with their interpretation as iterated `and` / `or`:

| Quantifier | Predicate results across elements | Result |
|------------|----------------------------------|--------|
| `forall` | All `true` | `true` |
| `forall` | Any `false` | `false` |
| `forall` | No `false`, some `undef` | `undef` |
| `forall` | Empty collection | `true` (vacuous truth) |
| `exists` | Any `true` | `true` |
| `exists` | All `false` | `false` |
| `exists` | No `true`, some `undef` | `undef` |
| `exists` | Empty collection | `false` (vacuous falsity) |

When the collection itself is `undef`, quantifiers produce `undef`.

#### 9.2.7 Function Application

`undef` propagates strictly through function calls. If any argument is `undef`, the result is `undef`. Functions are pure and cannot inspect the determinacy state of their arguments.

**Exception:** Determinacy predicates (`determined()`, `constrained()`, `undetermined()`, `partially_determined()`) operate on the determinacy state itself, not the value. `determined(undef_param)` returns `false`, not `undef`.

#### 9.2.8 `Option` Constructors

`some(undef)` is valid and distinct from both `none` and `undef`:

| Expression | Meaning |
|------------|---------|
| `some(5mm)` | Value present, determined |
| `some(undef)` | Value present, content not yet decided |
| `none` | Value absent |
| `undef` (of type `Option<T>`) | Existence itself not yet decided |

`Option<Option<T>>` is a valid type. `some(none)`, `some(some(x))`, `none`, and `undef` are all distinguishable states. Pattern matching on `Option<Option<T>>`:

```
match outer {
    some(some(x)) => ...    // Inner value present
    some(none) => ...       // Outer present, inner absent
    none => ...             // Outer absent
}
```

#### 9.2.9 Tracing

Tooling should make it easy to trace why a value is `undef` -- which upstream parameter's undetermined state is responsible. This is an implementation concern, not a language semantics concern.

### 9.3 `auto` Resolution

- A decision, not an absence.
- **Strict `auto` (default):** Resolved value must be uniquely determined or uniquely optimal. If not, resolution failure with diagnostic.
- **Free `auto`:** Returns a feasible solution, warns about non-uniqueness.

**`auto` for type parameters:** Valid -- means "pick a type that satisfies the bounds." See §3.9 and `docs/auto-type-param-resolution.md` for the resolution algorithm (per-parameter BFS, cap of 10, lexicographic tiebreak by FQN).

**Interaction with `undef`:** If some parameters are `undef` (not `auto`), the optimizer's result for `auto` parameters is conditional on the `undef` parameters. When `undef` parameters later become determined, dependent `auto` resolutions are invalidated and re-resolved.

**Tiebreaking rule:** When multiple values are equally optimal, a deterministic tiebreaking rule applies (e.g., lexicographic ordering, closest to conventional default). The specific rule matters less than it being deterministic and documented.

### 9.4 Determinacy Predicates

Boolean predicate functions (compiler intrinsics, in prelude):

- `determined(param)` -- the parameter has a specific value
- `constrained(param)` -- the parameter has at least one constraint applied
- `undetermined(param)` -- the parameter is `undef` with no constraints
- `partially_determined(param)` -- constrained and not determined

These compose with `forall`, `exists`, `and`, `or`, participate in `where` guards.

### 9.5 Purposes

A purpose is a named determinacy predicate -- requirements specifying which parameters must be determined for a particular downstream use to be viable. Purposes are activatable -- when active, their constraints and outputs are present; when deactivated, absent.

```
purpose fits_within(part : Structure, envelope : Structure) {
    let clearance = envelope.min_wall - part.max_extent
    constraint clearance > 0mm
    constraint forall p in part.geometric_params: determined(p)
    where exists p in part.material_params: constrained(p) {
        constraint forall p in part.material_params: determined(p)
    }
    minimize part.mass
}
```

### 9.6 Computation Failures

For v0.1, computation failures are evaluation-graph-level events, NOT language-level values.

When a computation fails:
1. The node's result is marked `Failed` (variant of `Freshness` enum).
2. A realization event with `EventKind::error` is emitted.
3. Downstream nodes become `Pending` with a diagnostic chain.
4. The UI surfaces failures through existing diagnostics.

**No `Result<T, E>` type. No `try`/`catch`. No language-level error propagation.**

**Freshness enum (4 variants):**

```
Freshness:
    | Final
    | Intermediate { generation: u64 }
    | Pending { last_substantive: ResultRef }
    | Failed { error: ErrorRef }
```

---

## 10. Constraint System

### 10.1 First-Class Constraints

Constraints are first-class entities: named, parameterized, composed, inherited, collected into libraries. They are predicates -- they assert truth.

The `@optimized` hook indicates a definition has a semantically equivalent optimized implementation. Language-level definition is in terms of language primitives; the optimized implementation may use a specialized solver.

```
@optimized("geo_kernel::coincidence_solver")
constraint def Coincident(a : Point3<Length>, b : Point3<Length>) {
    distance(a, b) == 0mm
}
```

### 10.2 The Checking -> Solving -> Proposing Spectrum

1. **Checking:** Given a fully determined design, verify all constraints hold.
2. **Solving:** Given a partially determined design with `auto` parameters, find values satisfying all constraints.
3. **Proposing:** Given a highly underdetermined design, provide useful feedback about what is constrainable, what is in conflict, and what would need to be determined to make progress.

Three modes form a hierarchy with graceful degradation.

### 10.3 Constraint Domains

| Domain | Description |
|--------|-------------|
| Dimensional/parametric | Numeric relationships between typed, dimensioned scalar parameters |
| Geometric | Spatial relationships (coincidence, parallelism, tangency) |
| Logical/combinatorial | Discrete choices, boolean gating, type selection |
| Cross-domain | Span multiple domains simultaneously (e.g., DFM rules) |

The constraint engine is an orchestrator dispatching to specialized sub-solvers, not a monolithic solver.

### 10.4 Optimization

Optimization is unified with constraint solving. `minimize`/`maximize` are syntactic sugar:

```
structure def LightweightBracket : Rigid {
    param thickness : Length = auto
    param material : Material = auto
    constraint thickness >= 2mm
    minimize mass
}
```

**Multi-objective:** Weighted sum is the default. Lexicographic ordering is an explicit extension. Pareto exploration is a tooling concern, not a language-level construct.

**Discrete, continuous, and mixed problems:** All supported. `auto` may search continuous (wall thickness), discrete (bolt selection), or both. Solver orchestrator dispatches based on parameter types.

### 10.5 Scope-Level Objectives

Optimization objectives are scoped to the containing entity. Narrowest scope wins.

```
structure def System {
    minimize total_cost                      // System-level
    sub bracket : Bracket {
        minimize mass                        // Subsystem-level
    }
    sub housing : Housing {
        // No local objective -- inherits system-level
    }
}
```

### 10.6 Bottom-Up Resolution

Default resolution strategy:

1. Resolve `auto` parameters in leaf scopes using local objectives.
2. Treat resolved leaf scopes as fixed.
3. Resolve `auto` parameters in parent scopes using parent's objectives.
4. Continue upward to root.

Bottom-up is an approximation when scopes are coupled. Implementation should detect coupling and surface diagnostics.

### 10.7 Default Objectives

If no explicit purpose or objective is specified, a default purpose applies (provided by standard library). Expected default: robustness-oriented -- among feasible values, prefer those maximizing distance from constraint boundaries (centrality in the feasible region).

**Legibility:** Designer can always query what objective governs a given `auto` resolution.
**Override:** Any scope can override with a local `minimize`/`maximize`.

**Domain library smart defaults via `@solver_hint`:** Domain libraries can register smarter defaults for specific parameter types via the `@solver_hint` annotation (e.g., bolt length snaps to next standard size, sheet thickness snaps to available stock). Unlike `@optimized` (which provides a semantically equivalent fast path), `@solver_hint` changes solver behavior -- it provides domain-specific guidance about feasible values, preferred search strategies, or discrete candidate sets. See Section 12.1.

**Conflicting objectives:** Two objectives in the same scope that conflict without weighting = error. Designer must combine into weighted objective or establish lexicographic priority.

---

## 11. Standard Library Overview

### 11.1 Module Tree

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
```

### 11.2 Prelude Contents

See Section 7.6 for full listing.

### 11.3 Module Summaries

The complete API reference for all `std.*` modules is in the [Standard Library Reference](reify-stdlib-reference.md). Brief summaries:

| Module | Purpose |
|--------|---------|
| `std.math` | Numeric functions (`abs`, `sqrt`, `clamp`), trigonometry (`sin`, `cos` -- take `Angle`), linear algebra (`dot`, `cross`, `normalize`), complex numbers |
| `std.units` | 34 named dimension aliases, SI units with prefixes, imperial units, physical constants (`pi`, `g`, `c`, `boltzmann`) |
| `std.geometry` | Primitive constructors (box, cylinder, sphere), boolean operations, sweeps, transforms, patterns, spatial queries, topology selectors, geometric traits (`Geometry`, `Transformable`, `Bounded`, `Watertight`) |
| `std.structural` | Physical structure traits (`Physical`, `Rigid`, `Flexible`, `ElasticallyDeformable`) |
| `std.ports` | Port trait hierarchy: mechanical (`Bore`, `Shaft`, `RotaryPort`), electrical (`PowerPort`, `SignalPort`), thermal, fluid. Directionality and compatibility rules |
| `std.materials` | Material trait hierarchy: mechanical (`Elastic`, `Strong`, `Ductile`), thermal, electrical, optical, chemical properties |
| `std.tolerancing` | Dimensional tolerances, geometric tolerances (GD&T: flatness, position, runout, etc.), surface finish |
| `std.process` | Manufacturing process traits (`Subtracting`, `Adding`, `Forming`, `Joining`), DFM rule framework |
| `std.io` | Boundary abstractions (`Source`, `Sink`, `Input`, `Buy`, `Output`, `Discard`), format occurrences (STEP, STL, 3MF) |
| `std.analysis` | Analysis trait, stress post-processing (`von_mises`, `safety_factor`) |
| `std.fields` | Field interpolation, spatial operations (`compose`, `sample`, `restrict`), differential operators (`gradient`, `divergence`, `curl`, `laplacian`) |
| `std.determinacy` | Determinacy predicates (in prelude), utility constraints (`AllParamsDetermined`), example purposes (`design_review`, `simulation_ready`) |

**Language-semantic notes** (details that affect type system or compilation, retained here):

- **Dimensional `sqrt`:** `sqrt` is a compiler intrinsic that halves even exponents. `pow` with non-integer exponents is restricted to dimensionless in v0.1. `pow` with integer literal exponents on dimensioned quantities works through repeated multiplication.
- **Trig functions take `Angle`** (not `Real`), enforcing the 8th-dimension distinction.
- **Port compatibility rules:** `In` <-> `Out` (valid), `Bidi` <-> anything (valid), `In` <-> `In` (type error), `Out` <-> `Out` (type error).
- **Patterns return `List`** for per-instance constraints; compose with `union_all` for merged solid.
- **`scale` is non-rigid** -- does not compose with `Transform<3>`.

---

## 12. Annotations and Pragmas

### 12.1 Annotations

Annotations use `@name` or `@name(arguments)`. They provide hints to the toolchain.

**`@optimized`** -- registers that a language-level definition has a semantically equivalent optimized implementation in the runtime. The optimized implementation must produce identical results; it is a pure performance optimization:

```
@optimized("geo_kernel::coincidence_solver")
constraint def Coincident(a : Point3<Length>, b : Point3<Length>) {
    distance(a, b) == 0mm
}
```

**`@solver_hint`** -- provides domain-specific guidance to the constraint solver. Unlike `@optimized`, this *may change solver behavior* -- it narrows search spaces, provides discrete candidate sets, or suggests preferred strategies:

```
@solver_hint("discrete_set", standard_bolt_lengths)
param length : Length = auto

@solver_hint("prefer_stock", sheet_stock_thicknesses)
param thickness : Length = auto
```

The language-level definition remains the specification of correctness; the solver hint influences *how* the solver searches, not *what* constitutes a valid solution. Solver hints are advisory -- the solver may ignore them if they conflict with constraints.

**`@deprecated`** -- marks a definition as deprecated with a message:

```
@deprecated("Use RevisedBracket instead")
structure def OldBracket { ... }
```

**`@test`** -- marks a constraint definition or structure definition as a test case. Tests are the mechanism for regression testing of engineering designs: verifying that safety factors hold, tolerances are met, and design intent is preserved across changes.

**Test declarations:**

```
@test
constraint def TestBoltStrength {
    sub bolt : M8Bolt { grade = 10.9, length = 25mm }
    bolt.proof_load >= 40kN
    bolt.yield_strength >= 900MPa
}

@test
constraint def TestBracketSafetyFactor {
    sub bracket : Bracket { thickness = 3mm, width = 50mm, material = Steel_1045 }
    safety_factor(bracket.stress_field, bracket.material.yield_strength).min >= 2.0
}

@test
structure def TestAssemblyFit {
    sub housing : Housing { bore_diameter = 25mm }
    sub shaft : Shaft { diameter = 24.98mm }
    connect housing.bore -> shaft.journal : ClearanceFit
    constraint housing.bore.diameter > shaft.diameter
    constraint housing.bore.diameter - shaft.diameter < 0.1mm
}
```

**Test semantics:**
- A test is a self-contained scope. Sub-structures instantiated in a test are local fixtures -- they do not affect the containing module's design.
- Bare constraint expressions in a `@test constraint def` body are assertions. All must hold for the test to pass.
- A `@test structure def` passes if all its constraints are satisfied and no computation failures occur.
- Tests are never part of the evaluation graph during normal design use. They are evaluated on demand by the test runner.

**Test discovery and execution:**
- The test runner discovers all `@test`-annotated declarations in the specified modules.
- Tests are independent and may run in parallel.
- A test passes if all constraints are `satisfied`. A test fails if any constraint is `violated`. A test is `indeterminate` if any required parameter is `undef` (missing fixture data).
- Test output reports pass/fail with constraint diagnostics for failures.

**What can be asserted in tests:**
- Parameter values and relationships (`bolt.length == 25mm`)
- Constraint satisfaction (`safety_factor(...) >= 2.0`)
- Determinacy state (`determined(bracket.thickness)`)
- Geometric properties (`volume(part.geometry) < 100cm^3`)
- Connection compatibility (via test structures with `connect` statements)

### 12.2 Pragmas

Pragmas use `#name(arguments)` and are scoped to the enclosing block. They are toolchain directives, not part of the semantic model. Pragmas never change the meaning of a program -- only its implementation characteristics.

**`#precision`** -- hint to toolchain about numeric precision:

```
#precision(float64)
```

**`#solver`** -- solver preference for constraints in scope:

```
#solver(nlopt, algorithm = LD_SLSQP)
```

**`#no_prelude`** -- suppress the implicit prelude import:

```
#no_prelude
```

**`#kernel`** -- override implicit kernel dispatch:

```
#kernel(occt)
```

**`#version`** -- declare the target language version (see Section 14.2):

```
#version(0.1)
```

---

## 13. Documentation Generation

Reify source files produce structured documentation from two mechanisms: **doc comments** (`///`) and **`meta` blocks**.

### 13.1 Doc Comments

Doc comments (`///`) are attached to the immediately following declaration. They produce API documentation -- hover text, reference pages, inline help.

```
/// A standard hex-head bolt per ISO 4014.
///
/// The bolt length includes the head. Thread length is computed
/// from the nominal length per ISO 888 Table 3.
pub structure def HexBolt : Fastener {
    /// Nominal bolt diameter (shank diameter).
    param diameter : Length
    /// Total bolt length including head.
    param length : Length
}
```

Doc comments support a minimal markup:
- Blank `///` lines separate paragraphs.
- Inline code uses backticks: `` `parameter_name` ``.
- No other formatting in v0.1.

### 13.2 `meta` Blocks in Documentation

`meta` block entries (Section 4.8) are included in generated documentation as structured metadata -- part numbers, revision codes, compliance references. They are displayed separately from the prose description.

### 13.3 Generated Output

The documentation tool generates structured output (format implementation-defined) containing:
- Declaration name, kind, type parameters, trait list
- Doc comment prose
- Parameter table (name, type, default, doc comment)
- Port table (name, type, direction, doc comment)
- Constraint summaries
- `meta` block entries
- Module hierarchy and cross-references

Documentation generation is a toolchain feature, not a language semantic. The language defines the source annotations; the toolchain defines the output format.

---

## 14. Language Versioning and Stability

### 14.1 Version Scheme

Reify uses semantic versioning for the language specification: `MAJOR.MINOR.PATCH`.

- **MAJOR** (0 -> 1): Language stabilization. Breaking changes are expected during the 0.x series.
- **MINOR** (0.1 -> 0.2): New features, possible breaking changes to unstable features.
- **PATCH** (0.1.0 -> 0.1.1): Bug fixes, clarifications, non-breaking additions.

### 14.2 Source File Versioning

Every `.ri` file may declare the language version it targets:

```
#version(0.1)
module my_project.bracket
```

If omitted, the file is assumed to target the toolchain's current version. The `#version` pragma is advisory in v0.1 -- full version-gated parsing is deferred.

### 14.3 Stability Guarantees (v0.1)

v0.1 is a **draft specification**. No backwards compatibility guarantees are made between v0.1 and any future version. Users should expect breaking changes.

The following are expected to be stable across the 0.x series:
- Core syntax shape (curly-brace declarations, `param`/`port`/`sub`/`let`/`constraint` member kinds)
- Dimensional analysis model (10 base dimensions, quantity literals with units)
- Determinacy spectrum (`undef`/constrained/`auto`/determined)
- Module system structure (one file = one module, `pub`/private visibility)

The following may change:
- Standard library API surface (trait hierarchies, function signatures)
- Keyword set (additions likely, removals possible)
- Grammar details (operator precedence, syntactic sugar)
- Annotation and pragma set

### 14.4 Standard Library Versioning

The standard library evolves independently of the language specification. Its API surface is documented in the [Standard Library Reference](reify-stdlib-reference.md).

Policy for `std.prelude`: Additions are acceptable; removals and semantic changes are breaking. All other `std.*` modules may evolve freely during the 0.x series.

### 14.5 Migration

When a breaking change is introduced, the toolchain should provide:
1. A diagnostic identifying the affected construct and the migration path.
2. Where feasible, an automated migration tool (`reify migrate --from 0.1 --to 0.2`).

Detailed migration guides are published with each minor version release.

---

## 15. Grammar Summary

Complete EBNF grammar incorporating all updates from all documents and design review resolutions.

```ebnf
(* === Top-level === *)
file            ::= module_decl? import* declaration*

module_decl     ::= 'module' module_path

import          ::= 'pub'? 'import' import_path ('as' IDENT)?
import_path     ::= module_path ('.' '{' IDENT (',' IDENT)* '}')?
                   | module_path '.' TYPE_IDENT

module_path     ::= IDENT ('.' IDENT)*

(* === Declarations === *)
declaration     ::= visibility? (entity_decl | trait_decl | fn_decl | purpose_decl
                                | enum_decl | unit_decl | let_decl | type_alias_decl)
                   | connect_stmt | chain_stmt | meta_block

visibility      ::= 'pub'

(* --- Entity declarations --- *)
entity_decl     ::= entity_kind 'def' TYPE_IDENT type_params? trait_list?
                     where_clause? '{' member* '}'

entity_kind     ::= 'structure' | 'occurrence' | 'constraint' | 'field'

(* --- Trait declarations --- *)
trait_decl      ::= 'pub'? 'trait' TYPE_IDENT type_params?
                     (':' trait_ref ('+' trait_ref)*)? where_clause?
                     '{' trait_member* '}'

trait_member    ::= param_decl | port_decl | sub_decl | let_decl | constraint_line
                   | assoc_type_decl

assoc_type_decl ::= 'type' TYPE_IDENT (':' trait_bound)?         (* associated type *)

(* --- Function declarations --- *)
fn_decl         ::= 'pub'? 'fn' IDENT type_params? '(' fn_params? ')' '->' type_expr
                     '{' fn_body '}'

fn_params       ::= fn_param (',' fn_param)*
fn_param        ::= IDENT ':' type_expr

fn_body         ::= (let_decl)* expr

(* --- Type alias declarations --- *)
type_alias_decl ::= 'pub'? 'type' TYPE_IDENT type_params? '=' type_expr

(* --- Purpose declarations --- *)
purpose_decl    ::= 'pub'? 'purpose' IDENT type_params?
                     '(' purpose_params? ')' '{' purpose_member* '}'

purpose_params  ::= purpose_param (',' purpose_param)*
purpose_param   ::= IDENT ':' type_expr

purpose_member  ::= constraint_line | sub_decl | let_decl | minimize_decl | maximize_decl

minimize_decl   ::= 'minimize' expr
maximize_decl   ::= 'maximize' expr

(* --- Enum declarations --- *)
enum_decl       ::= 'pub'? 'enum' TYPE_IDENT '{' enum_variant (',' enum_variant)* '}'
enum_variant    ::= TYPE_IDENT

(* --- Unit declarations --- *)
unit_decl       ::= 'unit' IDENT ':' type_expr ('=' expr)? ('offset' expr)? ('scale' expr)?

(* === Type expressions === *)
type_params     ::= '<' type_param (',' type_param)* '>'
type_param      ::= TYPE_IDENT (':' trait_bound)? ('=' type_expr)?
                   | IDENT ':' kind ('=' expr)?

trait_bound     ::= trait_ref ('+' trait_ref)*
trait_list      ::= ':' trait_ref ('+' trait_ref)*
trait_ref       ::= type_expr

kind            ::= 'Nat' | 'Dimension'

type_expr       ::= TYPE_IDENT type_args?
                   | type_expr '*' type_expr
                   | type_expr '/' type_expr
                   | type_expr '^' expr
                   | '(' type_expr (',' type_expr)+ ')'
                   | type_expr '->' type_expr
                   | module_path '.' TYPE_IDENT type_args?

type_args       ::= '<' type_arg (',' type_arg)* '>'
type_arg        ::= type_expr | expr

where_clause    ::= 'where' expr (',' expr)*            (* boolean expressions *)

(* === Members === *)
member          ::= param_decl | port_decl | sub_decl | let_decl | type_alias_decl
                   | constraint_line | connect_stmt | chain_stmt
                   | entity_decl | field_body | fn_decl
                   | where_block | match_block | meta_block
                   | minimize_decl | maximize_decl

where_guard     ::= 'where' expr                         (* per-declaration guard *)

param_decl      ::= 'param' IDENT ':' type_expr ('=' expr)? where_guard?
port_decl       ::= 'port' IDENT ':' dir? type_expr ('{' member* '}')? where_guard?
sub_decl        ::= 'aux'? 'sub' IDENT ':' type_expr where_guard? ('{' member* '}')? ('at' expr)?
let_decl        ::= 'pub'? 'aux'? 'let' IDENT (':' type_expr)? '=' expr where_guard?
constraint_line ::= 'constraint' (constraint_ref | expr) where_guard?

constraint_ref  ::= TYPE_IDENT type_args? '(' args? ')'

dir             ::= 'in' | 'out'

field_body      ::= 'source' '=' field_source '{' field_content '}'
field_source    ::= 'analytical' | 'sampled' | 'composed' | 'imported'
field_content   ::= (param_assign | lambda)*

(* === Statements === *)
connect_stmt    ::= 'connect' port_ref connect_op port_ref
                     (':' type_expr)? connect_block?
connect_op      ::= '->' | '<->'
connect_block   ::= '{' (param_assign | port_mapping)* '}'
param_assign    ::= IDENT '=' expr
port_mapping    ::= IDENT '->' IDENT

chain_stmt      ::= 'chain' IDENT ('->' IDENT)+

port_ref        ::= path ('@' IDENT ('(' args ')')? )?

where_block     ::= 'where' expr '{' member* '}' ('else' '{' member* '}')?

match_block     ::= 'match' expr '{' match_arm* '}'
match_arm       ::= pattern '=>' (member+ | expr)
pattern         ::= TYPE_IDENT ('|' TYPE_IDENT)*
                   | 'some' '(' IDENT ')'
                   | 'none'
                   | '_'

meta_block      ::= 'meta' '{' meta_entry* '}'
meta_entry      ::= IDENT '=' STRING_LIT

(* === Expressions === *)
expr            ::= literal
                   | IDENT
                   | path
                   | expr binop expr
                   | unop expr
                   | expr '(' args? ')'
                   | expr '[' expr ']'
                   | expr '.' IDENT
                   | expr '.' '(' trait_ref '::' IDENT ')'
                   | TYPE_IDENT '::' IDENT
                   | lambda
                   | conditional
                   | quantifier
                   | match_expr
                   | 'undef'
                   | 'auto' ('(' 'free' ')')?
                   | 'some' '(' expr ')'
                   | 'none'
                   | 'self'
                   | '(' expr ')'

quantifier      ::= ('forall' | 'exists') IDENT 'in' expr ':' (expr | connect_stmt | constraint_line)

lambda          ::= '|' lambda_params? '|' expr
lambda_params   ::= lambda_param (',' lambda_param)*
lambda_param    ::= IDENT (':' type_expr)?

conditional     ::= 'if' expr 'then' expr 'else' expr

match_expr      ::= 'match' expr '{' match_expr_arm* '}'
match_expr_arm  ::= pattern '=>' expr

path            ::= IDENT ('.' IDENT)*

args            ::= arg (',' arg)*
arg             ::= (IDENT '=')? expr

binop           ::= '+' | '-' | '*' | '/' | '%' | '^'
                   | '==' | '!=' | '<' | '>' | '<=' | '>='
                   | 'and' | 'or' | 'implies'
                   | '..' | '..<'

unop            ::= '-' | 'not'

(* === Literals === *)
literal         ::= INT_LIT
                   | REAL_LIT
                   | BOOL_LIT
                   | STRING_LIT
                   | quantity
                   | range_lit
                   | list_lit
                   | set_lit
                   | map_lit

quantity        ::= (INT_LIT | REAL_LIT) unit_expr
unit_expr       ::= IDENT (('*' | '/') IDENT)* ('^' INT_LIT)?
                   | '(' unit_expr ')' ('^' INT_LIT)?

range_lit       ::= expr '..' expr
                   | expr '..<' expr
                   | ('<' | '<=' | '>' | '>=') expr

list_lit        ::= '[' (expr (',' expr)*)? ']'
set_lit         ::= 'set' '{' (expr (',' expr)*)? '}'
map_lit         ::= 'map' '{' (map_entry (',' map_entry)*)? '}'
map_entry       ::= expr '=>' expr

BOOL_LIT        ::= 'true' | 'false'
```

### 13.1 Newline and Continuation Rules

Reify is newline-significant: newlines terminate declarations and statements within `{ }` blocks.

**Continuation inside `()` and `[]`:** Newlines are whitespace. Free continuation.

**Inside `{}`:** Newlines separate declarations and statements.

**Trailing continuation:** A line ending with any of the following continues to the next line:
- Binary operators: `+`, `-`, `*`, `/`, `%`, `==`, `!=`, `>=`, `<=`, `>`, `<`, `and`, `or`, `implies`
- Connection operators: `->`, `<->`
- Comma: `,`
- Opening delimiters: `(`, `[`, `{`

**Leading continuation:** A line beginning with any of the following continues the previous line:
- Logical operators: `and`, `or`, `implies`

Other leading operators do NOT continue the previous line. Use parentheses or trailing-operator style.

**No backslash continuation.** The combination of free continuation inside `()`/`[]` and trailing-operator continuation covers all practical cases.

---

## 16. Appendix: Operator Precedence Table

From highest to lowest precedence:

| Precedence | Operator(s) | Associativity | Description |
|------------|-------------|---------------|-------------|
| 1 (highest) | `.` | Left | Member access |
| 2 | `::` | Left | Qualified trait access |
| 3 | `@` | Left | Ad-hoc port selector |
| 4 | `[i]` | Left | Indexing |
| 5 | `f(x)` | Left | Function call |
| 6 | `^` | Right | Exponentiation |
| 7 | `-` (unary) | Prefix | Negation |
| 8 | `*` `/` `%` | Left | Multiplication, division, modulo |
| 9 | `+` `-` | Left | Addition, subtraction |
| 10 | `..` `..<` | Non-assoc | Range |
| 11 | `==` `!=` `<` `>` `<=` `>=` | Left (chainable) | Comparison |
| 12 | `not` | Prefix | Logical negation |
| 13 | `and` | Left | Logical conjunction |
| 14 | `or` | Left | Logical disjunction |
| 15 (lowest) | `implies` | Right | Logical implication |

Chained comparisons: `a < b < c` desugars to `a < b and b < c`.

---

## 17. Appendix: Complete Keyword List

Alphabetical listing of all v0.1 keywords:

```
and          as           auto         chain        connect
constraint   def          else         enum         exists
false        field        fn           forall       if
implies      import       in           let          map
match        maximize     meta         minimize     module
none         not          occurrence   or           out
param        port         pub          purpose      self
set          some         structure    sub          then
trait        true         type         undef        unit
where
```

**Total: 46 keywords.**

---

## 18. Appendix: Items Deferred to v0.2+

| # | Item | Target | Notes |
|---|------|--------|-------|
| 1 | Default robustness objective | v0.1.1 | Mechanism depends on constraint solver internals |
| 2 | Rich structural query/traversal | v0.2 | `children`/`members` pseudo-collection filterable by trait |
| 3 | Geometry selector strengthening | v0.2 | Persistent naming, advanced topological queries |
| 4 | `Result<T>` or `fallback` expressions | v0.2 | Language-level error handling |
| 5 | Associated `fn` in traits | v0.2+ | Procedural code in traits |
| 6 | Data-carrying enums | v0.2+ | Algebraic data types with associated values |
| 7 | Tolerance stack-up analysis | Realized (v0.6) | `stackup_worst_case` / `stackup_rss` / `monte_carlo_stackup` eval builtins; v1 is explicit-chain only (assembly-graph auto-derivation deferred). See docs/prds/v0_6/tolerance-stackup-analysis.md. |
| 8 | Keyed collection identity | v0.2 | Named/keyed members in collections instead of positional |
| 9 | Field-valued material properties | v0.2 | `Field<Temperature, Pressure>` for temperature-dependent properties |
| 10 | Expression-body sugar for `fn` | Deferred | `fn f(x: T) -> T = expr` shorthand |
| 11 | No `Date` type | v0.1 | ISO 8601 strings used for timestamps |
| 12 | No `priv` modifier | Deferred | Hidden parameters on public definitions not yet justified |
| 13 | Conditional compilation | Deferred | Conditional imports, platform-specific module variants |
| 14 | String interpolation | Deferred | Display/templating concern |
| 15 | Complex number literal syntax | Deferred | `3.2 + 4.1j` sugar |
| 16 | `AffineMap` type for non-rigid transforms | Deferred | Scaling, shearing transforms |
| 17 | Differential operators full implementation | v0.1+ | `@optimized`; may be partial in early versions |
