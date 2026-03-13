# Language Ontology: Design Decisions

**Status:** Foundation complete — ready for type system and syntax design  
**Version:** 0.1 — First crystallization from exploratory design sessions

---

## 1. Design philosophy

The language is a text-based domain-specific language for expressing physical parts and systems, targeting mechanical and mechatronic design, simulation, manufacturing (including assembly process modelling, additive manufacturing, and CAM).

**Core design principles:**

- **Elegance:** Fundamental simplicity, orthogonality, composability, and modularity.
- **Expressive power:** Simple to express simple things; no more difficult than necessary to express arbitrarily complex things.
- **Fluidity:** Support the full spectrum from abstract sketch to manufacturing-ready specification within a single unified framework.
- **Human-LLM co-authoring:** The primary authoring mode. Syntax must be concise, unambiguous, and structurally regular enough for reliable LLM generation while remaining natural for human engineers.
- **Clean-slate design:** Not built on existing standards, but designed to bridge to them (STEP, FMI, 3MF, etc.) as needed.
- **Minimal core, rich libraries:** The language core should be as small, powerful, and general as possible. Domain complexity (GD&T, DFM rules, kinematic joint types, material databases) belongs in libraries developed with strong community involvement.

---

## 2. Entity types

The language has four fundamental entity types. These are the primitive ontological categories — everything expressible in the language is one of these four things.

### 2.1 Structure

**What it is:** Anything that exists in space. Parts, sub-parts, features, connectors, assemblies (as compositions of structures), material regions, geometric bodies.

**Key properties:**

- Structures compose spatially — a structure can contain sub-structures.
- Structures are immutable *within the design system*. They represent a snapshot of a physical configuration. If a manufacturing process transforms a structure, it produces a new structure; it does not mutate the original.
- Structures are freely editable *by the designer* in the source text. Version history of the design is managed externally (e.g., Git), not by the language itself.
- There is no distinction between "Part" and "Assembly." A structure that contains sub-structures is simply a composite structure. Whether a given structure is manufactured as one piece or assembled from multiple pieces is determined by the occurrences (processes) that produce it, not by its structural type. This avoids an arbitrary categorical distinction that is contingent on manufacturing context and point of view.
- A structure need not have geometry. A substanceless connector (e.g., a shrink fit connection, a magnetic coupling) is a legitimate structure with ports and constraints but no physical volume.

**Design rationale:** The Part/Assembly unification was driven by the principle that the divide is arbitrary and context-dependent. A PCB is a "part" to a system integrator but an "assembly" to the board house. A 3D-printed consolidated assembly is a single structure produced by a single occurrence. Forcing a categorical split leads to restrictions ("you can do that with parts but not assemblies") that contradict the language's design principles.

**Alternatives considered:**
- Distinct Part and Assembly types (rejected: creates arbitrary restrictions, contingent on manufacturing PoV)
- Artifact-centric model with Function/Form/Behavior/Material/Geometry facets, as in NIST CPM (rejected as the top-level organising principle, though these facets may appear as aspects or traits of structures)

### 2.2 Occurrence

**What it is:** Anything that happens in time. Manufacturing processes, assembly operations, simulations, tests, inspections, heat treatments, surface finishing.

**Key properties:**

- Occurrences transform structures. An occurrence consumes input structures and produces output structures.
- Occurrences compose sequentially — process chains are sequences of occurrences where each step's output feeds the next step's input.
- Occurrences can branch and merge (a casting process produces a structure; parallel machining operations modify different features; assembly brings them together).
- Intermediate structures between occurrence steps exist implicitly. They need not be named unless referenced.
- Occurrence definitions are abstract and loosely coupled to structures. A `Boring` operation is defined in terms of abstract feature types (`CylindricalFeature`), not specific parts. Binding to specific geometry occurs separately in a process plan. This matches how manufacturing engineering works in practice (process planning is a separate activity from part design) and enables process reuse across parts.

**The fundamental insight:** "Assembly is a process." The act of bringing structures together is an occurrence, not a structural category. This is what dissolves the Part/Assembly distinction — a structure is a structure regardless of whether it was machined from a billet, printed as one piece, or assembled from fifty sub-structures.

**Relationship between structures and occurrences:** Structures and occurrences are distinct ontological categories. Occurrences transform structures. We don't have to express the complete process — we can simply declare "this is a complete space shuttle." But if we want to describe *how* it comes into existence, we do so through occurrences, and the language integrates that description with the structural design. "Welding is a process. A weld is a structure."

**Alternatives considered:**
- Structure/Occurrence as a spectrum rather than a binary split (rejected: the categories are genuinely distinct, and blurring them loses the clean semantics of "occurrences transform structures")
- Tight coupling between occurrences and structures, where processes directly reference specific parts (rejected: reduces reusability and conflates design and process planning)

### 2.3 Constraint

**What it is:** A logical predicate over typed parameters. Constraints express geometric relationships, physical laws, dimensional requirements, manufacturing feasibility rules, kinematic restrictions, temporal ordering, and any other logical assertion about the design.

**Key properties:**

- Constraints are first-class entities. They can be named, parameterised, composed, inherited, and collected into libraries.
- Constraints compose logically — primarily via conjunction (multiple constraints applied together must all hold), but the language provides full logical connectives (`and`, `or`, `not`, `implies`, `forall`, `exists`).
- Constraints apply to both structures and occurrences. Geometric parallelism constrains structural geometry; temporal precedence constrains occurrence ordering; DFM rules constrain the relationship between structural features and manufacturing occurrences.
- Constraints are predicates, not relationships. A constraint asserts that something must be true. It does not by itself carry semantic meaning about *why* (design intent). Design intent is expressed through the type system — the *type* of a connection or the *type* of a constraint bundle communicates meaning.
- Inline constraints (within a structure or occurrence definition) are syntactic sugar for defining and immediately applying a named constraint.
- Library constraints enable reusable rule sets (e.g., `DFM_Milling`, `GDT_Position`, `MinWallThickness`).

**Minimal bootstrap set for the constraint system:**
- Equality and inequality over all types (atomic predicates)
- Logical connectives: `and`, `or`, `not`, `implies`, `forall`, `exists`
- Arithmetic over physical quantities with units
- Set/collection operations: `all`, `any`, `count`, `sum`
- Geometric primitives: `distance`, `angle`, `cross`, `dot`, `on`, `intersects`

Everything else (GD&T, kinematic joints, DFM rules, temporal ordering) is built from these primitives in the standard library. The language specification does not attempt to enumerate all possible constraint types — that is the library's job. The spec provides the minimal, general-purpose foundation.

**Optimised implementations:** The language should provide a general mechanism (a hook or annotation) indicating that a particular definition has an optimised implementation available. This applies to *anything* expressible in the language — not just constraints. The language spec defines the semantics; the implementation provides the fast path. Examples: a geometric constraint solver with built-in recognition of coincidence, tangency, and parallelism; a structure generator with an optimised microstructure filler; a faithful deep simulation of a physically complex occurrence such as hydroforming or composite layup. In every case, the *language-level* definition is in terms of the language's own primitives, and the optimised implementation is a semantically equivalent substitute registered via the hook mechanism.

**Alternatives considered:**
- Constraints as attributes of other entities, not first-class (rejected: prevents constraint composition, libraries, and cross-entity constraints)
- Constraints as a cross-cutting "aspect" concern (rejected: too implicit; first-class entities with syntactic sugar for inline use provides both power and convenience)

### 2.4 Field

**What it is:** A mapping from a domain to a codomain. Fields represent continuous (or discontinuous) functions over spatial, temporal, parametric, or abstract domains.

**Key properties:**

- Fields are first-class entities with identity. They can be named, composed, constrained, passed as parameters, and referenced by other entities.
- Fields are generic over their domain and codomain types:
  - **Domain:** spatial (ℝ³), spatiotemporal (ℝ³ × ℝ), parametric (e.g., UV surface space), 1D (along a beam axis), N-dimensional, or abstract (index spaces).
  - **Codomain:** scalar (temperature, density), vector (displacement, velocity), tensor (stress, strain), discrete (material ID, lattice type), boolean (inside/outside predicates), or any other type.
- Fields compose via arithmetic, logic, and functional operations: addition, scaling, interpolation, thresholding, conditional selection.
- Fields need not be continuous. Binary material assignments, step functions at boundaries, and discrete lattice type selectors are all valid fields.
- Geometry itself can be represented as a field: an implicit/SDF representation is a scalar field from ℝ³ → ℝ where the zero-level-set defines the surface.
- Fields can parameterise both structures and occurrences. A spatially varying density field controls lattice infill. A spatial power-map field controls laser scan strategy in LPBF. A temporal temperature profile field parameterises a heat treatment occurrence.

**Design rationale:** This is the nTopology insight made general. Simulation-driven spatial variation of geometry parameters (lattice density driven by stress, wall thickness driven by thermal load) is the future of design-analysis integration. Making fields first-class and general (not limited to spatial scalar fields) enables the language to express this natively. No other existing DSL has this as a first-class concept.

**Note on Constraint/Field structural similarity:** A constraint is a boolean-valued function over its parameters. A field is a function over a domain. A constraint is therefore structurally similar to a boolean field. This deep similarity may matter for the type system, but the design intent of the two concepts is sufficiently different (asserting requirements vs. describing spatial/temporal variation) that they are kept as separate entity types.

---

## 3. Axes of variation

Every entity (Structure, Occurrence, Constraint, Field) has a position along two orthogonal axes of variation.

### 3.1 Abstraction (type refinement)

**What it is:** Taxonomic depth — how refined the *kind* of thing is.

- A `Fastener` is abstract.
- A `Bolt` is more concrete (a kind of Fastener).
- A `HexBolt` is more concrete still (a kind of Bolt).
- An `M8HexBolt` is very concrete (a specific standard).

Abstraction is about *what kind of thing* this is, independent of whether all its parameters have values. You can have a very abstract, fully determined entity ("a fastener with all fastener-level properties specified") or a very concrete, underdetermined entity ("an M8 hex bolt with unspecified length").

**Mechanism:** Abstraction is expressed through the type system via trait refinement (see §5 Type System).

### 3.2 Determinacy (parametric specificity)

**What it is:** How much of the entity's parameterisation is determined — how many of its parameters have specific values, constraints, or delegations.

Determinacy subsumes what was originally conceived as two separate axes (parametric specificity and completeness). If the language models everything as parameters — including optional sub-structure presence (a boolean parameter gating a structural branch), taxonomic choices (type parameters), and geometric configuration — then determinacy is the single axis tracking how much is settled.

**The determinacy spectrum for any parameter:**

1. **`undef`** — Nothing has been said. This is the default state of every parameter that hasn't been assigned, constrained, or delegated. Epistemic honesty: "we don't know yet."
2. **Constrained but not determined** — The parameter has constraints narrowing its domain, but no single value. For example: `wall_thickness > 2mm`. Something has been said, but the parameter isn't resolved.
3. **`auto`** — Delegated to the system (compiler, solver, AI, or other tooling). This is a *decision*, not an absence — it says "I want this to have a value; figure it out for me." Always syntactically valid. The system may respond with a resolved value, or with "I can't determine this — here's what I'd need."
4. **Determined** — A specific value or fully resolved expression. `wall_thickness = 3mm`.

**Key design decision:** `undef` is the default. If you declare a parameter and don't assign it, it's `undef`. No ceremony needed. This is critical for supporting early-stage design where most things are unspecified.

**Propagation semantics:**
- `undef` propagates: if a computation depends on an `undef` parameter, the result is `undef` (to the extent that the result relevantly depends on the undefined input).
- `auto` does not propagate in the same way — it's a request for resolution, and the resolver either succeeds (producing a determined value) or fails (producing a diagnostic).
- Constrained parameters propagate their constraint sets through dependent computations.

**Determinacy is relative to purpose.** A structure might be fully determined for stress analysis but underdetermined for manufacturing. This is handled through named determinacy predicates (see §6 Purposes).

**Alternatives considered:**
- Separate axes for "parametric specificity" and "completeness" (rejected: these converge if everything is modelled as parameters; determinacy is the unified concept)
- Ternary axis including `auto` as a midpoint (rejected: `auto` is a special value orthogonal to the `undef`→constrained→determined spectrum, not a point on it)

---

## 4. Definition and usage

**Definition** creates a type at some position in abstraction × determinacy space. It is optionally parameterised with types and/or instance values.

**Usage** (instantiation) creates an instance from a type by filling in some or all of the parameters. Both types and instances occupy positions within the space defined by the axes of variation.

This is a language construction mechanism, not an axis of variation. A `def` introduces a reusable template; a usage instantiates it in context, potentially further refining (increasing abstraction specificity) and further determining (increasing determinacy) it.

```
// Definition: a type with parameters
structure def Bracket {
    parameter thickness : Length
    parameter material : Material
    // ...
}

// Usage: an instance with some parameters determined
bracket1 : Bracket { thickness = 5mm, material = Steel_304 }

// Usage: a less determined instance
bracket2 : Bracket { thickness = 5mm }  // material is undef
```

Definitions can be nested and can themselves be parameterised by other definitions (generic/parameterised types), enabling type families.

---

## 5. Type system

### 5.1 Traits

The language uses **traits** as its sole abstraction and composition mechanism for types. There is no class inheritance (single or multiple).

**What a trait is:** A named, composable bundle of requirements — ports, parameters, and constraints that any implementing type must satisfy. Traits are the mechanism for expressing both taxonomic hierarchies and cross-cutting capabilities.

**Trait refinement** expresses taxonomic depth:
```
trait Fastener {
    port head_side : MechanicalPort
    port thread_side : MechanicalPort
    parameter rated_load : Force
}

trait Threaded : Fastener {
    parameter thread_pitch : Length
    parameter thread_diameter : Length
}

trait BoltShaped : Threaded {
    parameter shank_length : Length
    parameter head_type : HeadType
}
```

**Trait composition** expresses cross-cutting capabilities:
```
structure def CopperBusbar : RigidMechanical + Conductive {
    material: Copper_C110
    // Must satisfy all requirements from both traits
}
```

**Why traits, not inheritance:**
- Traits compose because they're constraints, not containers. Two traits can both require a `max_temperature` parameter — it's the same parameter satisfying both requirements. Conflicts arise only when two traits impose contradictory constraints on the same parameter, which is a genuine design error that should be flagged.
- Traits model physical reality correctly. A copper busbar doesn't "inherit from" a conductor — it *satisfies the requirements of being* a conductor. Traits express "can do" relationships, which better match how physical capabilities work than "is a" hierarchies.
- Composition over inheritance: implementation reuse is achieved by composing sub-structures and delegating trait requirements to them, rather than inheriting implementation from a base type.
- Single point of composition mechanism: no need to distinguish "extending a class" from "implementing an interface." Everything is trait implementation, which is simpler.

**What traits replace:**
- Multiple inheritance → trait composition (`A + B + C`)
- Single inheritance → trait refinement (`trait Specific : General`)
- Interfaces → traits *are* the interface/contract mechanism
- Mixins → traits with default parameter values

If practical experience demonstrates that single inheritance would do significant additional useful work beyond what traits provide, it may be added later. The current assessment is that every use case for inheritance is better served by trait refinement (for taxonomic hierarchies) or sub-structure composition with delegation (for implementation reuse).

### 5.2 Dimensional analysis (planned)

Compile-time unit checking is a planned core feature of the type system. Details to be specified in the type system design phase.

### 5.3 Port types and connection typing (planned)

Ports are typed interaction points on structures. Port types are defined via traits. Connection compatibility is enforced through the type system — `connect` verifies that the two ports' types are compatible and that the connector type (if specified) satisfies the requirements of both ports. Details to be specified in the type system design phase.

---

## 6. Composition and connection

### 6.1 Spatial composition

Structures compose spatially by containment — a structure can contain sub-structures. This is the primary mechanism for building complex designs from simpler components.

### 6.2 Connection

**`connect`** is a language construct (not an entity type) that creates a typed connection between two ports.

**What `connect` does (it is syntactic sugar for all of the following at once):**
1. Creates a **Connector structure** at the interface (the physical coupling, if any — or a substanceless semantic marker)
2. Generates **constraints** between the connector and both ports, derived from port type compatibility
3. Registers **topology** — the connection edge in the design graph

**Syntax:**
```
connect A -> B                           // connector type = auto
connect A -> B : ShrinkFit               // connector type specified
connect A -> B : ShrinkFit { ... }       // connector type with parameters
connect A -> B : M8Bolt { grade = 10.9 } // concrete connector
```

**Substanceless connectors are legitimate.** A `ShrinkFitConnector` has ports and constraints (interference range, contact pressure, assembly temperature differential) but no geometry. This is not a degenerate case — it's a normal structure whose geometry parameter is empty. The principle: a Connector structure requires ports and constraints; geometry is one parameter among many, and like all parameters, it can be `undef`, `auto`, or empty.

**Uniform topology:** Every connection creates a Connector structure, even substanceless ones. This guarantees uniform traversal of the design graph — query "what is connected to what" by traversing connector structures. No special cases for implicit connections.

**Relationship semantics come from types, not tags.** The *meaning* of a connection (load-bearing, alignment, sealing, electrical, thermal) is expressed through the type system: the types of the ports, the type of the connector, and the traits they implement. Tags/metadata are an antipattern — they can't be composed, checked, or reasoned about by the type system.

**If you create the same entities and constraints manually (without `connect`), you achieve the same design effect** — `connect` is pure sugar for clarity and well-formedness guarantees. The underlying model is just structures + constraints + topology.

### 6.3 Sequential composition

Occurrences compose sequentially — process chains are sequences where each step's output feeds the next step's input, connected via `connect` on occurrence ports.

---

## 7. Special values

### 7.1 `undef` (undefined)

- Meaning: "We have not decided, or don't know."
- Default state: every unassigned parameter is `undef`.
- Propagation: `undef` propagates through dependent computations. If a computation depends on an `undef` parameter, the result is `undef` (to the extent it relevantly depends on the undefined input). `undef` propagation may be contained/swallowed where downstream computation does not depend on the undefined value.
- Always valid: it is always syntactically and semantically valid for a parameter to be `undef`.
- Tracing: understanding the chain of `undef` propagation is an important UX/implementation concern. The tooling should make it easy to trace *why* something is `undef` and *what would need to be determined* to resolve it.

### 7.2 `auto` (delegated)

- Meaning: "The design system (compiler, modeler, solver, AI, or other tooling) should determine this, without bothering me with the details any more than it must."
- This is a *decision*, not an absence. It explicitly delegates authority to the system.
- Always syntactically valid: you can say `auto` for any parameter.
- Resolution: the system either resolves it to a determined value, or responds with a diagnostic ("I can't determine this — here's what I'd need"). The handling of `auto` is an important axis of variability between implementations and configurations.
- Smoothly resolving ambiguities brought about by extensive or high-level use of `auto` is an extremely important implementation detail at all layers.

### 7.3 Interaction with determinacy

The determinacy spectrum for any parameter is: `undef` → constrained → `auto` → determined. However, `auto` is not strictly "more determined" than constrained — it is orthogonal. A parameter can be both constrained and `auto` (meaning: "figure it out, but it must satisfy these constraints"). The resolution of `auto` must respect all applicable constraints.

A parameter is **fully determined** when it has a specific value. A parameter is **fully indeterminate** when it is `undef` with no constraints. Everything else is in between.

---

## 8. Bridging concepts and determinacy management

### 8.1 Port

A typed interaction point or region of a structure. Usually but not necessarily on the boundary. Ports define *how* a structure interacts with its context — what flows it can accept or provide, what geometric interfaces it exposes, what constraints connection implies.

Ports are defined within structure definitions (they are not independent entities — a port without a parent structure is meaningless). However, port *types* are defined via traits and have their own type hierarchy (e.g., `MechanicalPort`, `ElectricalPort`, `FluidPort`, `ThermalPort`), their own parameters (rated voltage, max flow rate, thread specification), and can compose into interfaces (an `Interface` is a bundle of ports — e.g., "NEMA 17 mounting face" = 4 bolt holes + pilot bore + shaft port).

### 8.2 Purpose (determinacy predicates)

**The problem:** Determinacy is not a single scalar. A structure might be fully determined for stress analysis but underdetermined for manufacturing. Determinacy is relative to a downstream use.

**The mechanism:** A purpose is a named determinacy predicate — a set of requirements specifying which parameters must be determined (or constrained to what degree) for a particular downstream use to be viable.

**Implementation note:** Purpose is most likely a library-level pattern built on constraints, not a core language primitive. A purpose definition is essentially a constraint over the determinacy states of a set of parameters. The specificity and complexity of particular purposes (what exactly does "manufacturable" require for a specific process?) belongs in domain libraries where it can be refined by practitioners.

**Dependency graph navigation:** If the design's dependency graph is computed, each node provides a natural point of view for completeness queries. Root nodes (entities that depend on others but are not depended upon) commonly correspond to significant design outputs (a manufacturable instance, a simulation, a concrete artifact). A dependency navigator that can query and display determinacy from any node's perspective, and facilitate navigation around the graph, is a powerful design tool. Determinacy from any given perspective is less like a scalar and more like a stack trace — it shows not just *how complete* something is, but *why* it's incomplete (which dependencies are unresolved, and what they in turn depend on).

### 8.3 State

**The model:** A structure doesn't *have* a state — it *is* a state. A structure is an immutable snapshot of a physical configuration. Occurrences consume and produce structures, modeling transformation over time.

This approach was chosen because it cleanly handles cases that are awkward under "structures have mutable state":
- Cutting a bar in half: the occurrence `Cut` consumes one `Bar` structure and produces two `Bar` structures. No identity crisis.
- Heat treatment: the occurrence `HeatTreat` consumes `Part{state: annealed}` and produces `Part{state: tempered}`. Same type, different structure.
- Assembly: the occurrence `Assemble` consumes N structures and produces one composite structure.
- Machining: the occurrence `Mill` consumes a workpiece and produces a modified-geometry structure.

This gives the language functional/immutable semantics for structures within the design system, while the source text remains freely editable by the designer. Version history is managed externally (Git), not by the language.

---

## 9. Summary of key distinctions

| Concept | Is a... | Is not a... |
|---|---|---|
| A bolt | Structure | — |
| A weld bead | Structure | — |
| A shrink fit connection | Structure (substanceless) | — |
| Welding | Occurrence | Structure |
| Assembly | Occurrence | Structure type |
| Parallelism requirement | Constraint | Relationship |
| A bolted joint | Connection (connector structure + constraints) | A separate entity type |
| Temperature distribution | Field (ℝ³ → Scalar) | An attribute of a structure |
| "Is this manufacturable?" | Purpose (determinacy predicate) | A property of a structure |
| "We haven't decided yet" | `undef` | An error |
| "The system should decide" | `auto` | `undef` |
| A HexBolt | Structure implementing BoltShaped trait | A subclass of Bolt |

---

## 10. Open questions for subsequent design phases

### 10.1 Type system details (next phase)
- Built-in types: geometric types, physical quantity types, the unit system
- Compile-time dimensional analysis design
- Trait definition syntax and semantics in detail
- Port type hierarchy and connection compatibility rules
- How generic/parameterised type definitions work

### 10.2 Syntax design (next phase)
- Concrete surface grammar
- Keyword vocabulary
- Block structure and delimiters
- Expression syntax for constraints and field composition
- Module/namespace system

### 10.3 Constraint system details
- Solver integration architecture
- Over-constraint and under-constraint detection and diagnostics
- Constraint priority/preference for soft constraints vs. hard constraints
- The hook mechanism for optimised implementations (general, not constraint-specific)

### 10.4 Field system details
- Field definition syntax (analytical, sampled, composed)
- Field-to-geometry integration (implicit/SDF as fields)
- Field visualisation and export

### 10.5 Standard library architecture
- What ships in the core vs. community libraries
- Versioning and compatibility
- Extension mechanisms

---

*Document generated from design exploration sessions. Intended as a living specification to be refined through subsequent design phases.*
