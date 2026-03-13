# Type System: Design Decisions

**Status:** Foundation complete — ready for syntax design, constraint system details, and field system details  
**Version:** 0.1 — First crystallization from type system design sessions  
**Builds on:** `ontology-design-decisions.md` v0.1

---

## 1. Design approach

The type system was designed bottom-up in four layers, each building on the previous:

1. **Primitive layer** — scalars, physical quantities, dimensional analysis, geometric types, collections
2. **Parameterisation layer** — type parameters, value parameters, bounds, inference, defaults
3. **Abstraction layer** — trait semantics, composition, conflict resolution, conformance
4. **Connection layer** — port types, connection compatibility, ad-hoc connections

---

## 2. Primitive types

### 2.1 Bare scalars

| Type | Notes |
|---|---|
| `Bool` | Predicates, flags, gating of optional sub-structures |
| `Int` | Counts, indices, discrete quantities. Distinct from `Real` — promotes to `Real` implicitly but not the reverse |
| `Real` | General-purpose numeric type. Dimensionless real number. Precision is an implementation concern, not a language-level type distinction |
| `String` | Names, labels, identifiers, human-readable descriptions |

**Key decision:** `Int` and `Real` are separate types. A bolt count is categorically different from a wall thickness. The type system catches continuous/discrete confusion. Precision (float32/float64/arbitrary) is abstracted away at the language level — the toolchain decides based on context. If explicit precision control is ever needed (GPU kernels, mesh export), it belongs in an annotation/pragma system.

### 2.2 Physical quantities and dimensional analysis

**Core model:** Dimensions are part of the type. Units are part of the literal syntax and value representation. Two quantities with the same dimension and different units are the same type. The type checker operates on dimensions; unit conversion is automatic.

**Dimension representation:** A dimension is a vector of rational exponents over 8 base dimensions (7 SI + Angle):

```
[Length, Mass, Time, Current, Temperature, Amount, Luminosity, Angle]

Length       = [1, 0, 0, 0, 0, 0, 0, 0]
Force        = [1, 1, -2, 0, 0, 0, 0, 0]   // M·L·T⁻²
Pressure     = [-1, 1, -2, 0, 0, 0, 0, 0]  // M·L⁻¹·T⁻²
Torque       = [1, 1, -2, 0, 0, 0, 0, -1]  // M·L·T⁻²·Angle⁻¹ (distinct from Energy)
```

Multiplication of quantities adds exponent vectors. Division subtracts. Checked at compile time with zero runtime cost.

**Angle as 8th base dimension:** Angles are dimensionless in SI (radians = m/m), but treating them as dimensionless is a known source of engineering errors. The torque/energy confusion (`N·m` vs `N·m`) is the canonical example. Adding Angle as an 8th base dimension catches `torque + energy` as a type error. Cost: trig functions need explicit typing (`sin : Angle → Dimensionless`), and radian/steradian conventions need explicit handling. This cost is justified by the error-catching benefit.

**Named dimension aliases:** Provided for readability. These are type aliases, not new types — `Force` and `Mass * Length / Time^2` are the same type:

```
dimension Length = [1, 0, 0, 0, 0, 0, 0, 0]
dimension Force  = Mass * Length / Time^2
dimension Pressure = Force / Length^2
```

**Deferred:** Unit literal surface syntax and compound unit notation — deferred to syntax design phase.

### 2.3 Geometric types

**The Point/Vector distinction (affine space):** Points and vectors are different types. Points are positions (affine space elements). Vectors are displacements (vector space elements). The algebraic rules are enforced by the type system:

- `Point - Point → Vector` ✓
- `Point + Vector → Point` ✓
- `Vector + Vector → Vector` ✓
- `Point + Point` → type error ✗

**Parameterisation:** Geometric types are parameterised by spatial dimensionality and quantity:

```
Point<N: Nat, Q: Dimension>    // Position. Q is typically Length.
Vector<N: Nat, Q: Dimension>   // Displacement / physical vector.
```

Type aliases for common cases:

```
Point2<Q> = Point<2, Q>
Point3<Q> = Point<3, Q>
Vector2<Q> = Vector<2, Q>
Vector3<Q> = Vector<3, Q>
```

Dimensionality is parameterised (not separate types like `Point2`/`Point3`) because spatiotemporal and higher-dimensional modeling may be needed.

**Geometric types carry physical dimensions.** A `Point3<Length>` is a position in physical space. A `Vector3<Force>` is a force vector. This unifies dimensional analysis with geometry and catches errors like adding a force vector to a position.

**Tensor type:**

```
Tensor<Rank: Nat, N: Nat, Q: Dimension>
```

All indices range over the same spatial dimension N. Transforms covariantly/contravariantly under coordinate changes.

- `Vector<N, Q> = Tensor<1, N, Q>` (rank-1 alias)
- `Scalar<Q> = Tensor<0, N, Q>` (rank-0 alias — dimensioned number)
- `Point<N, Q>` is **NOT** a tensor — it is a separate affine-space type

`Matrix<M: Nat, N: Nat, Q: Dimension>` is a separate type for general M×N rectangular arrays. `Tensor<2, N, Q>` implicitly converts to `Matrix<N, N, Q>` but not the reverse (an arbitrary square matrix isn't necessarily a physics tensor with correct transformation properties).

**Tensor symmetry:** Expressed via trait (`Symmetric`), not type parameter. Symmetry is a constraint on a tensor's values, and traits are the mechanism for expressing constraints on types. This also allows the implementation to optimize storage (6 vs. 9 components for a symmetric 3×3) based on trait satisfaction.

**Orientation:** An opaque type representing rotation in N-dimensional space. The language does not commit to a representation (quaternion, rotation matrix, Euler angles, axis-angle). Construction from any common representation is supported:

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

- `Transform` is always rigid (rotation + translation). Non-rigid maps (scaling, shearing) are a separate type (`AffineMap` or similar).
- Sub-structure placement is a `Transform` from child frame to parent frame.
- Global position is computed by composing Transforms up the containment tree.
- There is no implicit global frame. All coordinates are relative to parent.
- Ports expose Frames (when geometrically located). Connections constrain Transforms between Frames.

**Frame projection operator:** Deferred until entity name resolution is designed. Expected to be straightforward at that point.

**Library-level geometric types:** `Plane`, `Axis`, `BoundingBox` — none require compiler/type-system magic. All are expressible as structures with standard members. Moved to standard library.

### 2.4 Collection types

| Type | Purpose |
|---|---|
| `List<T>` | Ordered sequence (bolt patterns, process step lists, point clouds) |
| `Set<T>` | Unordered unique collection (material options, feature sets) |
| `Map<K, V>` | Key-value mapping (material property tables, parameter lookups) |
| `Range<T>` | Bounded interval — `Range<Length>` for `2mm..5mm` tolerance ranges |
| `Option<T>` | Explicit optionality (present or absent). Distinct from `undef` — `Option` is a type-level statement about existence; `undef` is a determinacy state |

### 2.5 Complex numbers

`Complex<Q>` is a **standard library type**, not a core primitive. It is a structured type (pair of `Q` values) with arithmetic defined over it. The dimensional system composes naturally: `Complex<Impedance>` has real and imaginary parts both in impedance units.

Rationale: `Complex` doesn't need compiler magic the way dimensional analysis does. The optimised implementation hook (from ontology §2.3) allows the compiler to recognise and optimise it. Literal syntax (e.g., `3.2 + 4.1j`) can be added as sugar if needed.

### 2.6 `undef` and `auto` interaction with the type system

**Determinacy is tracked orthogonally, not baked into types.** Parameter types are written as plain `Length`, `Force`, etc. Determinacy (`undef` / constrained / `auto` / determined) is a property of the parameter tracked by the design system, not part of the type.

Rationale: making every parameter type `Maybe<Length>` would create enormous syntactic noise for little benefit, since `undef` is the normal state during early design. The type system knows about determinacy only at the level of purpose predicates (ontology §8.2), where you can ask "is this parameter determined enough for stress analysis?"

---

## 3. Parameterisation

### 3.1 Two kinds of parameters

**Type parameters** — parameterise over types. Resolved at definition time (compile time). Move along the **abstraction** axis. Different type parameter bindings produce different types.

**Value parameters** — parameterise over values. Resolved at instantiation time. Exist along the **determinacy** spectrum (`undef` → constrained → `auto` → determined). Different value parameter bindings produce different instances of the same type.

### 3.2 Syntax placement

Type parameters go in angle brackets. Value parameters go in the body:

```
structure def FlexibleCoupling<DriverPort: RotaryPort, DrivenPort: RotaryPort> {
    parameter max_torque : Torque
    parameter max_misalignment : Angle
}
```

Rationale: type and value parameters have different resolution semantics (compile-time vs. determinacy-spectrum). Separating them syntactically makes the distinction visible.

### 3.3 Type parameter bounds

- **Trait bound:** `T: SomeTrait` — T must implement SomeTrait
- **Kind bound:** `N: Nat`, `Q: Dimension` — T must be a member of a built-in kind
- **Composite bound:** `T: TraitA + TraitB` — multiple trait requirements

### 3.4 Value parameter constraints

Via the constraint system — inline constraints in the definition body:

```
structure def Bolt {
    parameter shank_diameter : Length
    parameter head_diameter : Length
    constraint head_diameter > shank_diameter
}
```

### 3.5 Where-clauses

For cross-parameter constraints at the definition level:

```
structure def Adapter<PortA: MechanicalPort, PortB: MechanicalPort>
    where PortA.rated_load >= PortB.rated_load
{ ... }
```

### 3.6 Type inference

**Conservative.** Infer type parameters when context unambiguously determines them. Never infer value parameters — the determinacy model handles "not yet specified" via `undef`/`auto`/constrained/determined.

### 3.7 Defaults

**Type parameter defaults:**

```
structure def Fastener<HeadStyle: HeadType = Hex> { ... }
```

**Value parameter defaults:**

```
structure def Bolt {
    parameter grade : Real = 8.8
}
```

**Three-way distinction for unspecified parameters:**

1. No default, not specified → `undef` (truly unknown)
2. Has default, not specified → default value (determined, conventional)
3. Explicitly `undef` → `undef` even if default exists (designer deliberately unsetting)

Explicit `undef` overrides the default.

### 3.8 `auto` for type parameters

`auto` is valid for type parameters. Meaning: "I want this to be some specific type; system, figure out which one."

```
bearing1 : Bearing<auto: Seal> { bore_diameter = 25mm }
// "Pick a seal type that satisfies Seal"
```

Type `auto` and value `auto` share the same language-level semantics (delegation to the system, subject to constraints) but differ in resolution characteristics: type `auto` involves searching a space of types rather than solving for a number, and implementations may treat it as higher-stakes and more interactive. Bounds and constraints narrow the search space; unconstrained type `auto` is valid but broad.

If the theoretical non-orthogonality of `auto` across abstraction and determinacy axes is ever bothersome, it can be re-distinguished as `auto_type` and `auto_value` with shared syntactic sugar `auto`.

### 3.9 Limited dependent typing

Value parameters of type `Int` and `Bool` can appear in type-level positions:

- Collection sizes constrained by value parameters
- Conditional presence of sub-structures gated by boolean parameters
- Array dimensions parameterised by integer values

This is a targeted set of rules for well-understood engineering patterns, not a general dependent type theory. To be extended if the limitations pinch in practice — not expected.

---

## 4. Trait semantics

### 4.1 What a trait contains

| Member kind | Description |
|---|---|
| **Parameters** | Required named parameters with types |
| **Ports** | Required interaction points |
| **Sub-structure slots** | Required contained sub-structures satisfying a trait |
| **Constraints** | Logical requirements on relationships between members |
| **Derived parameters** | Values computed from other members — both a requirement and a default definition, overridable |
| **Associated types** | Type-level members that implementing types must bind |

### 4.2 What traits do NOT contain

- **Geometry** — traits can require geometric parameters and constrain geometry, but geometric bodies belong to implementing structures
- **Identity or state** — traits are stateless bundles of requirements, never directly instantiated
- **Implementation logic** — no procedural code, no method bodies. Derived parameters have declarative expressions (formulas), not procedures. This keeps traits purely declarative.

The no-implementation-logic decision is a genuine trade-off. Implementation reuse is achieved through composition: common code lives in sub-structures that are composed into implementing types. If this pattern proves too limiting (e.g., if abstract companion structures carrying common code proliferate alongside traits), procedural code in traits will be reconsidered.

### 4.3 Trait composition: conflict resolution

**Same name, same type → merge silently.** A single member satisfies both trait requirements. This is the common case and physically correct — a component has one `max_temperature`, not two just because two traits require it.

**Same name, different type → error.** The designer must explicitly resolve via aliasing or qualified access (`Trait::member`).

**Constraint composition → conjunction.** All constraints from all composed traits must hold simultaneously. Unsatisfiable constraint sets are detected and reported by the compiler.

### 4.4 Default values in traits

Traits can provide defaults for parameters, derived parameters, and sub-structures:

```
trait StandardThread {
    parameter handedness : Handedness = Handedness.Right
}

trait Cylindrical {
    parameter diameter : Length
    parameter length : Length
    derived volume : Volume = pi * (diameter/2)^2 * length
}

trait Sealed {
    sub seal : Seal = ORing { material = Nitrile }
}
```

Implementing types can override any default. Overrides must preserve the member's type. Explicit `undef` overrides defaults (consistent with §3.7).

### 4.5 Trait refinement

Refinement expresses taxonomic depth (`trait Threaded : Fastener`). Rules:

- **Additive requirements** — refinement can only add requirements, never remove
- **Narrowing constraints** — can tighten parent constraints, never relax
- **Narrowing types** — can narrow associated types / sub-structure types to subtypes
- **Default overriding** — can override parent defaults
- **Multiple refinement** — `trait MechatronicActuator : MechanicalActuator + ElectricalDevice + Controllable` — same conflict resolution as composition

### 4.6 Conformance checking

**What conformance requires:**

1. All required members present
2. Types match or are subtypes of requirements
3. Constraints satisfiable given the implementation's parameters

**Conformance is interleaved with determinacy.** A fully `undef` structure trivially conforms (constraints are vacuously satisfiable). Constraints are checked as parameters become determined. Full conformance is only verifiable when all relevant parameters are determined. This maps onto purpose/determinacy predicates.

**Nominal + structural hybrid:** Explicit trait declaration (`: BoltShaped`) is the primary conformance mode — it's a design commitment. Structural conformance ("does this structure happen to satisfy Trait X?") is available as a query/analysis tool for tooling and AI-assisted design, but doesn't change the structure's type.

---

## 5. Ports and connections

### 5.1 Revised port model

**The base `Port` trait is minimal:**

```
trait Port {
    parameter direction : Directionality = bidi
}
```

No geometry required. Geometry is added by subtrait refinement:

- `LocatedPort : Port` — adds a `Frame<3>` (point-like interaction)
- `RegionPort : Port` — adds a region parameter (surface, volume, curve)
- Bare `Port` — no geometry at all (wireless, abstract connections)

**Port directionality:** Three values:

- `in` — receives/accepts (socket, female thread, bore)
- `out` — provides/inserts (plug, male thread, shaft)
- `bidi` — symmetric (flat mating face, butt weld, thermal contact)

Compatibility: `in`↔`out`, `bidi`↔anything, `in`↔`in` and `out`↔`out` are type errors.

### 5.2 Port geometric scope

Ports can describe interactions at any geometric scale:

- A point (frame)
- A curve (weld seam, seal contact line)
- A surface region (mating face, bonding area, antenna)
- A volume (radiation pattern, convective envelope)
- The entire boundary (painted surface)
- No geometric locus (WiFi, Bluetooth)

The port type hierarchy accommodates this full range.

### 5.3 Ports on structures and occurrences

**Structure ports** describe spatial interfaces — how things physically interact. **Occurrence ports** describe flow interfaces — what goes in and what comes out of a process:

```
occurrence def Welding {
    port workpiece_a : in StructurePort
    port workpiece_b : in StructurePort
    port result : out StructurePort
}
```

Sequential composition of occurrences is connection of occurrence ports.

### 5.4 Multi-domain ports and interfaces

Trait composition enables multi-domain ports:

```
trait HydraulicPort : FluidPort + MechanicalPort {
    parameter fitting_type : FittingStandard
}
```

Interfaces (port bundles) are traits — a set of ports in fixed spatial relationships:

```
trait NEMA17MountingInterface {
    port bolt_hole_1 : in ThreadedPort
    port bolt_hole_2 : in ThreadedPort
    // ...
    constraint bolt_pattern_diameter == 31mm
}
```

No new concept needed — an interface is a trait that requires multiple ports with geometric constraints.

### 5.5 Connection compatibility

Connection is mediated by a connector type. Compatibility has three layers:

1. **Directionality** — `in`↔`out`, `bidi`↔anything
2. **Port type compatibility** — the connector's ports must be type-compatible with both connected ports
3. **Parameter compatibility** — constraints from connector and port definitions must be satisfiable (thread specs match, loads sufficient, etc.)

No special compatibility mechanism exists. Compatibility = the type system (trait matching) + the constraint system (parameter and geometric constraints).

### 5.6 Interface-level connections

Connecting through a multi-port interface is sugar for multiple port-pair connections:

```
connect motor.NEMA17 -> mount.NEMA17 : NEMA17BoltedConnection
```

Ports are matched by name when both sides implement the same interface trait. When interfaces differ (adapters), explicit port mapping is required. Exact syntax deferred to syntax design phase.

### 5.7 Ad-hoc connections

Ports need not be pre-declared. The connection spectrum:

1. **Fully formal** — both sides have explicit ports with matching types
2. **One-sided** — one side has an explicit port, the other designates a surface/region at connection time
3. **Fully ad-hoc** — neither side has pre-declared ports; ports created at connection time

Ad-hoc port creation:

```
// Minimal — just topology, everything else undef
connect phone_case -> nut

// With connector type — ad-hoc ports typed by context
connect phone_case -> nut : Adhesive { type = JBWeld }

// With explicit ad-hoc geometry
connect phone_case@region(back_surface) -> nut@face(flat) : Adhesive { type = JBWeld }
```

Ad-hoc ports are real (exist in the design graph), typed by context, and can be refined later. This preserves uniform topology while eliminating friction for informal connections.

### 5.8 Connection topology

- **Nodes:** structures (any nesting depth). **Edges:** connector structures.
- Connection graph overlays the containment tree with lateral edges.
- Connector ownership: nearest common ancestor by default, designer override available.
- Cyclic graphs are supported (four-bar linkages, closed kinematic chains).

### 5.9 `connect` expansion

`connect A.port1 -> B.port2 : ConnectorType { params }` creates:

1. A connector structure (instance of ConnectorType)
2. Constraints between connector and both ports (from connector trait definition)
3. Frame alignment constraints (when ports are geometrically located)
4. A topology edge in the design graph

---

## 6. Open questions for subsequent design phases

### 6.1 Syntax design (next phase)
- Unit literal syntax and compound unit notation
- Concrete surface grammar for all type system constructs
- Ad-hoc port creation syntax (`@region`, `@face`, etc.)
- Interface connection and port mapping syntax
- Complex number literal syntax (if warranted)
- Module/namespace system

### 6.2 Constraint system details (next phase)
- Solver integration architecture
- Over-constraint and under-constraint detection
- Constraint priority (soft vs. hard)
- The optimised implementation hook mechanism

### 6.3 Field system details (next phase)
- Field definition syntax (analytical, sampled, composed)
- Field-to-geometry integration (implicit/SDF as fields)
- Interaction of fields with the tensor type (`Field<Point3, Tensor<2, 3, Pressure>>` for stress fields)

### 6.4 Entity name resolution
- Path-based reference to entities in the containment tree
- Instance identity vs. type identity for repeated sub-structures
- Frame projection operator (deferred from type system design)

---

*Document generated from type system design sessions. Intended as a living specification to be refined through subsequent design phases.*
