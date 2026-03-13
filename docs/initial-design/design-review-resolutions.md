# Design Review Resolutions

**Status:** Record of decisions from comprehensive design review session  
**Date:** 2026-03-13  
**Scope:** Cross-cutting review of all 16 design decision documents  
**Purpose:** Capture all decisions, corrections, and clarifications for incorporation into subsequent specification synthesis

---

## 1. Mechanical corrections across all documents

These are find-and-replace edits to be applied across all 16 design decision documents. Each corrects an inconsistency identified during review.

### 1.1 `occurrence` → `sub` for contained instances

**Documents affected:** `name-resolution-and-scoping-design-decisions.md`, `structural-graph-changes-design-decisions.md`, and any other document using `occurrence` as the keyword for instantiating a contained sub-entity within a parent body.

**Rule:** `sub` is the keyword for contained instances of any entity type (structures, occurrences, fields) within a parent body. `occurrence` is only used in `occurrence def` declarations — defining an occurrence type. When an occurrence is instantiated as a child of another entity, the keyword is `sub`.

**Example:**
```
// Incorrect:
occurrence motor : ElectricMotor { shaft_diameter = 8mm }

// Correct:
sub motor : ElectricMotor { shaft_diameter = 8mm }
```

### 1.2 `RigidMechanical` → `Rigid`

**Documents affected:** `syntax-design-decisions.md` (§10 examples), and any other document referencing `RigidMechanical`.

**Rule:** The `RigidMechanical` marker trait was removed in `standard-library-boundary-design-decisions.md` §14. All references should use `Rigid` from `std.structural.traits`.

### 1.3 `derived` → `let`

**Documents affected:** `ontology-design-decisions.md`, `type-system-design-decisions.md`, `syntax-design-decisions.md`, and any other document using the `derived` keyword.

**Rule:** The `derived` keyword was replaced by `let` in `deferred-syntax-items-design-decisions.md` §2. All occurrences of `derived` as a member keyword should be replaced with `let`. The grammar production `derived_decl` is replaced by `let_decl`.

### 1.4 `Export`/`Import` → `Output`/`Input`

**Documents affected:** `evaluation-graph-completion-design-decisions.md` §5.5, and any other document using `Export` or `Import` as occurrence trait names for design boundary crossings.

**Rule:** Renamed in `standard-library-boundary-design-decisions.md` §12.1 to avoid collision with the module-system `import` keyword. The module-level `import` keyword for name bindings is unaffected.

### 1.5 `Float` → `Real`

**Documents affected:** `module-system-design-decisions.md` §7.2 (prelude contents).

**Rule:** The type system defines the type as `Real` (`type-system-design-decisions.md` §2.1). The prelude listing should read `Real`, not `Float`.

### 1.6 `require` → `constraint` with determinacy predicates

**Documents affected:** `constraint-system-design-decisions.md` §7.2, `module-system-design-decisions.md` §4.4.

**Rule:** Already resolved in `deferred-syntax-completion-design-decisions.md` §4, but not all prior documents were updated. All uses of `require` in purpose definitions should be replaced with `constraint` using determinacy predicates (`determined()`, `constrained()`, `undetermined()`).

### 1.7 Currency as 9th dimension

**Documents affected:** `type-system-design-decisions.md` §2.2.

**Action:** Add Currency as the 9th base dimension to the dimension representation. The exponent vector becomes:

```
[Length, Mass, Time, Current, Temperature, Amount, Luminosity, Angle, Currency]
```

Currency units (`USD`, `GBP`, `EUR`, etc.) are declared with the `unit` keyword. All currency values within a project use constant conversion factors. Time-varying exchange rates are outside the design system's scope.

**Rationale:** Enables natural expressions like `25USD/kg` for cost estimation throughout the process and procurement framework. `Currency` composes with physical dimensions via multiplication and division like any other dimension. Nonsensical combinations (`USD^2`, `Currency * Currency`) are coherent at the type level but incorrect for any typed binding — exactly what the dimensional system is designed to detect, just as it detects `m^9`.

---

## 2. New language-level decisions

### 2.1 `trait` is a non-entity declaration kind

**Gap identified:** Traits are the sole abstraction mechanism but have no grammar production. The `entity_kind` production is `'structure' | 'occurrence' | 'constraint' | 'field'`, and `trait` does not appear.

**Decision:** `trait` is a declaration-level construct, not an entity type. The language has four entity types (structure, occurrence, constraint, field) and three non-entity declaration kinds (`fn`, `trait`, `purpose`). Traits have no identity, no determinacy state, and no evaluation graph presence. They are named, composable bundles of requirements.

**Grammar addition:**
```
declaration     ::= visibility? (entity_decl | trait_decl | fn_decl | purpose_decl)
                   | connect_stmt | chain_stmt

trait_decl      ::= 'pub'? 'trait' TYPE_IDENT type_params? (':' trait_ref ('+' trait_ref)*)?
                    where_clause? '{' trait_member* '}'
trait_member    ::= param_decl | port_decl | sub_decl | let_decl | constraint_line
```

**Cross-reference:** This formalises what was assumed throughout `ontology-design-decisions.md` §5, `type-system-design-decisions.md` §4, and all subsequent documents.

### 2.2 `purpose` is a named declaration with AST identity

**Gap identified:** Purpose was described as "syntactic sugar for scoped constraints" in `evaluation-graph-completion-design-decisions.md` §5.1, but the examples show it as a named, parameterised, reusable, activatable definition.

**Decision:** `purpose` is a named, parameterised declaration kind. It is semantically equivalent to creating a scope containing zero or more `constraint` declarations and/or `Output` occurrence instantiations applied to specific entities. It has AST identity. The runtime provides activation and deactivation mechanics through implementation-defined UX (GUI toggle, CLI flag, headless always-on mode, etc.). Diagnostics reference the purpose by name.

When activated, the constraints and outputs within the purpose are present in the evaluation graph. When deactivated, they are absent. The checking/solving/proposing mode falls out of input determinacy state, not explicit mode selection.

**Grammar addition:**
```
purpose_decl    ::= 'pub'? 'purpose' IDENT type_params? '(' purpose_params ')'
                    '{' purpose_member* '}'
purpose_params  ::= purpose_param (',' purpose_param)*
purpose_param   ::= IDENT ':' type_expr
purpose_member  ::= constraint_line | sub_decl | let_decl | minimize_decl | maximize_decl
```

**Cross-reference:** Updates `evaluation-graph-completion-design-decisions.md` §5, `constraint-system-design-decisions.md` §7, `deferred-syntax-completion-design-decisions.md` §4, `standard-library-completion-design-decisions.md` §3.

### 2.3 `let` for both value and type bindings

**Gap identified:** `dimension Pressure = Force / Area` uses an unspecified `dimension` keyword that functions as a type alias.

**Decision:** `let` is used for both value bindings and type aliases. No `dimension` keyword. No separate `type` keyword.

```
// Value binding (existing):
let volume = thickness * width * height

// Type alias (new):
let Pressure = Force / Area
let StressTensor = Tensor<2, 3, Pressure>
let Point3L = Point3<Length>
```

**Disambiguation:** The compiler determines from context whether the RHS is a type expression or a value expression. The PascalCase/snake_case naming convention provides visual disambiguation. `Force` and `Area` are type names (PascalCase), so `Force / Area` is a type-level dimension expression. If ambiguity arises from convention violation, the compiler flags it.

**Rationale:** One fewer keyword. `let X be Y` reads correctly regardless of whether Y is a type or value. The naming convention already provides strong visual signals. A separate `type` keyword can be added later if real ambiguity problems emerge, as a backward-compatible addition.

**Cross-reference:** Removes `dimension` from the keyword set. Updates `syntax-design-decisions.md` §3.3, `standard-library-boundary-design-decisions.md` §2 (unit declarations reference dimension aliases).

### 2.4 `forall` generalised to statements

**Gap identified:** With the unified List model for counted sub-structures, per-element connections require applying `connect` across collection elements. `forall` is currently expression-level only (produces `Bool`). `connect` is a statement. There is no way to express "connect every element."

**Decision:** `forall` is generalised to apply to both expressions (existing: produces `Bool`) and statements (`connect`, `constraint`). The token after the `:` disambiguates: if it's a keyword introducing a statement (`connect`, `constraint`), the `forall` generates one statement per element; if it's an expression, the `forall` produces a conjunction as before.

```
// Expression-level (existing): produces Bool
forall v in vents: v.spacing > 10mm

// Statement-level (new): generates one connect per element
forall v in vents: connect v.inlet -> housing.air_channel

// Statement-level (new): generates one constraint per element
forall v in vents: constraint v.mass < 50g
```

**Permitted statement kinds inside `forall`:** `connect` and `constraint` only for v0.1. Other statement kinds (e.g., `sub`) may be added if use cases emerge.

**Parsing impact:** Modest. The parser is already inside `forall` and can look at the next token to determine statement vs. expression. `connect` and `constraint` are keywords, so no ambiguity with expression-level `forall`.

**Interaction with `where` blocks:** `forall` over statements works inside `where` blocks:

```
where needs_cooling {
    sub vents : List<Vent>
    constraint vents.count == vent_count
    forall v in vents: connect v.inlet -> housing.air_channel
}
```

**Rationale:** Mechanical engineers read `forall v in vents: connect ...` as "connect every vent." The two uses of `forall` (assert-for-all and do-for-all) are not in tension — the first is a special case of the second where the "do" is asserting a predicate. This follows the project-wide precedent of covering multiple distinct computer-scientific concepts with a single construct when the user experience benefits.

**Cross-reference:** Updates `syntax-design-decisions.md` §5.3, grammar §11.

### 2.5 `where` blocks gain `else` clause

**Decision:** `where` blocks (not inline `where` on single declarations) support an `else` clause:

```
where needs_cooling {
    sub fan_mount : FanMount { ... }
    sub vents : List<Vent> { ... }
} else {
    sub passive_vents : List<PassiveVent> { ... }
}
```

**Desugaring:**
```
sub fan_mount : FanMount where needs_cooling { ... }
sub vents : List<Vent> where needs_cooling { ... }
sub passive_vents : List<PassiveVent> where !needs_cooling { ... }
```

**Scope:** `else` applies to `where` blocks only, not to inline `where` guards on single declarations. Inline `where`/`else` on a single declaration is syntactically awkward and not included.

**Nesting:** `else` composes with nested `where` blocks:
```
where needs_cooling {
    sub fan : Fan { ... }
    where high_airflow {
        sub secondary_fan : Fan { ... }
    }
} else {
    sub passive_vent : PassiveVent { ... }
}
```

**Grammar addition:**
```
where_block     ::= 'where' expr '{' member* '}' ('else' '{' member* '}')?
```

**Cross-reference:** Updates `deferred-syntax-items-design-decisions.md` §3, grammar §9.1.

### 2.6 `fn` recursion is explicitly permitted

**Gap identified:** The spec does not state whether `fn` (pure function) definitions may be recursive.

**Decision:** `fn` recursion is permitted. A `fn` may call itself. Mutual recursion between `fn` definitions is permitted. Infinite recursion is a runtime error (stack overflow), not a compile-time error — the compiler does not attempt termination checking on `fn` definitions.

**Rationale:** Recursion is essential for expressing algorithms over recursive data structures (tree traversals) and for generating collections programmatically (via recursive accumulation). Without recursion, `fn` would be unable to express many practical engineering computations.

**Cross-reference:** Updates `deferred-syntax-items-design-decisions.md` §6.

### 2.7 v0.1 enums are C-style (no associated data)

**Gap identified:** `enum` was identified as a language-level keyword in `standard-library-boundary-design-decisions.md` §2.1, but formal specification was deferred. The question of data-carrying variants was open.

**Decision:** v0.1 enums are simple named alternatives with no associated data:

```
enum Directionality { In, Out, Bidi }
enum FitType { Clearance, Transition, Interference }
enum HardnessScale { Rockwell_A, Rockwell_B, Rockwell_C, Brinell, Vickers }
```

`Option<T>` with `some(value)` / `none` remains compiler-intrinsic. If user-defined data-carrying enums are needed, they can be added as a backward-compatible extension post-v0.1.

**Exhaustiveness:** `match` on an enum must cover all variants (or use `_` wildcard). Missing variants are compile errors.

**Trait implementation:** Enums can implement traits. An enum variant is a value of the enum type, not a separate type.

**Cross-reference:** Formalises the open question in `standard-library-boundary-design-decisions.md` §13.2.

---

## 3. Collection model unification

### 3.1 Unified List model for counted sub-structures

**Problem identified:** The examples use `Vent[vent_count]` syntax for counted sub-structure arrays, but this syntax has no grammar production. It creates an implicit second container concept distinct from `List<T>`, with unclear operations, unclear interaction with the collection type system, and unclear boundary between "array of entities" and "list value."

**Decision:** Counted sub-structures use `List<T>` uniformly. The `[n]` syntax is removed. A counted sub-structure is declared as a `List` with a count constraint:

```
sub vents : List<Vent>
constraint vents.count == vent_count
```

This is semantically equivalent to the previous `Vent[vent_count]` notation but uses the standard collection type. All `List` operations work uniformly on sub-structure lists.

**Runtime optimisation:** The runtime is free to decompose a `List<Structure>` into per-element evaluation graph nodes for fine-grained incrementality. This is an implementation optimisation invisible at the language level — consistent with the "source text is canonical, implementation details are invisible" principle that drives the geometry engine design.

**Structure-controlling inference:** The runtime recognises `count == N` constraints on `List<Structure>` sub-declarations as structure-controlling. When the count changes, the schema re-elaboration mechanism (structural-graph-changes-design-decisions.md) handles the topology change. The designer writes uniform `List` semantics; the compiler infers the structural elaboration strategy.

**Positional identity:** v0.1 uses positional indexing for collection elements (`vents[0]`, `vents[1]`, ...). Shrinking removes from the end. Upgrade path to keyed identity via traits is unchanged from `structural-graph-changes-design-decisions.md` §8.1.

**Cross-reference:** Updates `structural-graph-changes-design-decisions.md` §2, §4.3, §11; grammar in `syntax-design-decisions.md` §11.

### 3.2 `List.generate(n, fn)` combinator

**Gap identified:** No mechanism to build a list from a computation. Bolt patterns, lattice point distributions, and other generated collections require programmatic construction.

**Decision:** `List.generate(count, fn)` is a standard library function that produces a `List<T>` by applying a function to indices 0..count-1:

```
let bolt_positions = List.generate(bolt_count, |i|
    point3(
        radius * cos(i * 2 * pi / bolt_count),
        radius * sin(i * 2 * pi / bolt_count),
        0mm
    )
)
```

**Placement:** `std.prelude` or a `std.collections` module (to be determined during synthesis).

### 3.3 Minimal collection operations for v0.1

**`List<T>`:** `count`, `sum` (where T is numeric), `map`, `filter`, `fold`, `all`, `any`, `contains`, `[i]` indexing, `generate(n, fn)`, `concat`.

**`Set<T>`:** `count`, `contains`, `union`, `intersection`, `difference`. Iteration via `forall`/`exists`.

**`Map<K,V>`:** `[key]` lookup, `keys`, `values`, `count`, `contains_key`. Iteration via `forall`/`exists` over entries.

**`Range<T>`:** `contains`, `lower`, `upper`, `span` (upper - lower).

**Indexing grammar addition:**
```
expr            ::= ... | expr '[' expr ']'
```

**Out-of-bounds / missing key:** Evaluation-graph-level failure (see §4.1), not a language-level exception.

**Cross-reference:** Extends `type-system-design-decisions.md` §2.4, `syntax-design-decisions.md` §5.8.

---

## 4. Error handling, equality, and runtime semantics

### 4.1 Computation failures as evaluation-graph-level events

**Gap identified:** No error handling model for runtime computation failures (I/O errors, division by zero, kernel failures).

**Decision:** For v0.1, computation failures are evaluation-graph-level events, not language-level values. When a computation fails:

1. The node's result is marked with a new `Currency` variant: `Failed`.
2. A realisation event with `EventKind::error` is emitted, carrying structured diagnostic information.
3. Downstream nodes that depend on the failed node become `Pending` with a diagnostic chain.
4. The UI surfaces the failure through the existing diagnostic infrastructure (constraint panel, event journal, determinacy stack traces).

No `Result<T, E>` type. No `try`/`catch`. No language-level error propagation. The evaluation graph handles failures the same way it handles `undef` propagation — downstream computation cannot proceed, diagnostics explain why.

**Currency enum update:**
```
Currency:
    | Final
    | Intermediate { generation: u64 }
    | Pending { last_substantive: ResultRef }
    | Failed { error: ErrorRef }
```

**Upgrade path:** If v0.2 needs user-catchable errors (e.g., "try this geometric operation, fall back to a simpler one"), `Result<T>` or a `fallback` expression can be added at that point.

**Cross-reference:** Updates `evaluation-graph-completion-design-decisions.md` §2.2.

### 4.2 Geometric equality: identity vs. equivalence

**Decision:** Two distinct operations:

**Identity equality (`==` on geometry):** Compares specification identity — same node in the evaluation graph. Cheap, exact, deterministic. This is the default meaning of `==` for opaque handle types (`Solid`, `Surface`, `Curve`, etc.). Used by the evaluation graph's content-hash cache.

**Geometric equivalence (`geo_equiv`):** Approximate tolerance-based check using Hausdorff distance:

```
fn geo_equiv(a: Geometry, b: Geometry, tolerance: Length) -> Bool
// Semantically: thicken(a, tolerance) contains b
//           AND thicken(b, tolerance) contains a
```

This is an `@optimised` library function — expensive, approximate, and explicitly requested. It is not the default meaning of `==`.

**Rationale:** Making `==` mean approximate equivalence would break transitivity (the fundamental property of equality). `a == b` and `b == c` would not guarantee `a == c` under tolerance. Identity equality preserves the equivalence relation. Geometric equivalence is a separate, explicitly approximate operation.

**Implementation note:** The AST-to-dataflow-graph transformation should perform common subexpression elimination, so that `union(box_a, box_b)` appearing twice with the same bindings produces a single node. This sidesteps kernel determinism concerns for the common case. The `geo_equiv` function handles the uncommon case where structurally distinct specifications must be compared for geometric equivalence.

**Cross-reference:** New specification item. Relates to `geometry-engine-design-decisions.md` §3.3 (opaque handles), `evaluation-graph-design-decisions.md` §6.4 (early cutoff).

### 4.3 `fn` purity and kernel determinism

**Observation documented:** Geometric operations typed as `fn` (pure, no side effects) are dispatched at runtime to geometry kernels that are inherently stateful. The `@optimised` bridge handles this. The design assumes that kernel operations are deterministic given identical inputs — i.e., that `union(a, b)` called twice with identical inputs produces the same result.

**Status:** This is an implementation-level assumption, not a language-level guarantee. Most geometry kernels are deterministic given identical inputs and identical floating-point environment. If a kernel violates this assumption, it is an implementation bug. The evaluation graph's content-hash cache correctness depends on this assumption.

**Mitigation:** CSE at the graph construction level ensures that identical expressions over identical bindings produce a single node, avoiding the question of cross-invocation determinism for the common case.

---

## 5. Port model refinement

### 5.1 Ports as uniform typed scopes

**Clarification identified:** The mechanism for accessing the structure flowing through an occurrence port was not specified. Structure ports and occurrence ports appeared to have asymmetric access patterns.

**Decision:** A port is a typed scope with members, uniformly across both structure ports and occurrence ports. Access is via dot notation, like everything else in the language.

**Structure ports** contain interface parameters defined by the port's trait:
```
trait RotaryPort : MechanicalPort {
    param frame : Frame3
    param rated_torque : Torque
    param rated_speed : AngularVelocity
    param torque : Torque         // actual torque at this interface
    param angular_velocity : AngularVelocity
}

// Access:
motor.shaft.rated_torque     // port parameter
motor.shaft.torque           // actual value, determined by constraint system
```

**Occurrence ports** contain a primary payload (the structure or field flowing through) plus any port-level parameters:
```
trait StructurePort : Port {
    param payload : Structure
}

// Access (explicit):
casting.result.payload       // the structure produced by casting

// Access (implicit deref — see §5.2):
casting.result               // same thing, when context expects a Structure
```

**Physical quantities at structure ports** (torque, force, voltage, current, etc.) are parameters on the port type, constrained by connections. The connection creates constraint equations relating the parameters of connected ports. The constraint system resolves the actual values. No new mechanism is needed — this is the Modelica pattern expressed through existing Reify constructs.

**Multi-aspect access** (force vs. velocity vs. current at a port) resolves to "which member of the port type?" The port type's trait hierarchy defines which members exist. `RotaryPort` has `torque` and `angular_velocity`. `ElectricalPort` has `voltage` and `current`. Dot notation provides access.

### 5.2 Implicit deref for single-payload ports

**Decision:** If a port type has exactly one member of a "transportable" type (`Structure`, `Geometry`, `Field`), that member is the port's **primary payload**. In expression contexts where the expected type matches the payload type, the port reference implicitly resolves to the payload.

```
// StructurePort has a single Structure-typed member (payload).
// In expression context expecting Structure:
constraint draft_angle(casting.result) > 0.5deg
// Equivalent to:
constraint draft_angle(casting.result.payload) > 0.5deg
```

**Scope of implicit deref:** Expression contexts only. In `connect` statements, the port reference always means the port itself, not its payload. This is unambiguous because `connect` syntactically requires port references.

**Multiple payloads:** If a port type has multiple Structure-typed (or Geometry-typed, or Field-typed) members, no implicit deref applies. All access must be explicit via dot notation.

**Rationale:** Similar to Rust's `Deref` trait or Haskell's newtype unwrapping. It is a general mechanism, not a special case for occurrence ports. The overwhelmingly common case is a single payload, and requiring `.payload` on every occurrence port access would be pure ceremony.

**Cross-reference:** Updates `type-system-design-decisions.md` §5 (ports), `syntax-design-decisions.md` §4.3 (occurrence declarations). Relates to `ontology-design-decisions.md` §2.2 (occurrences transform structures).

### 5.3 Intermediate structures in occurrence chains

**Clarification:** The structure flowing between connected occurrences is accessible via the port it flows through. No new syntax is needed. Occurrence ports already have names; the port's value *is* the intermediate structure.

```
sub casting : SandCasting { ... }
sub machining : CNCMilling { ... }

connect casting.result -> machining.workpiece

// The intermediate structure is the port's payload:
constraint draft_angle(casting.result) > 0.5deg
```

`casting.result` in expression context resolves to the structure on that port via implicit deref (§5.2). The spec should explicitly state: **a port reference in expression context evaluates to the port's primary payload when context expects a transportable type.**

**Cross-reference:** Clarifies `ontology-design-decisions.md` §2.2 ("intermediate structures... exist implicitly... need not be named unless referenced").

---

## 6. Structural introspection and generic queries

### 6.1 v0.1: compiler intrinsics only

**Decision:** For v0.1, generic structural queries (`AllParamsDetermined`, `AllGeometryDetermined`, `RepresentationWithin`) remain compiler intrinsics. User code cannot generically traverse the containment tree or query "all sub-structures satisfying trait X."

**Rationale:** A reflection/introspection API is a large design surface that would be premature to specify before implementation experience.

### 6.2 v0.2 consideration: structural query language

**Flagged for future:** A `children` or `members` pseudo-collection on structures, filterable by trait and quantifiable via `forall`/`exists`:

```
forall s in self.children where s : Rigid {
    constraint s.mass < 10kg
}
```

This makes the containment tree a queryable data structure at the language level. It is a natural extension of existing `forall` semantics.

**Inspiration sources:** XPath (XML structural queries), CSS selectors, and geometry query selectors (`@face`, `@region`). There is a natural synergy between structural traversal (navigating the containment tree) and geometric traversal (navigating the boundary representation). A unified query language for v0.2 could cover both, borrowing from XPath's axis model (parent, child, ancestor, descendant, sibling) applied to both the design hierarchy and geometric topology.

**Cross-reference:** Relates to `geometry-engine-design-decisions.md` §10.4 (geometric queries and selectors), `name-resolution-and-scoping-design-decisions.md` §3 (scope visibility rules).

---

## 7. Items flagged for v0.2 or later

These are not v0.1 decisions but are recorded here to ensure they are not lost.

| Item | Priority | Notes |
|---|---|---|
| **Default robustness objective** | v0.1.1 | Without it, strict `auto` fails in many practical cases. May need to be the headline feature of the first post-v0.1 release. Mechanism depends on constraint solver internals not yet specified. |
| **Rich structural query/traversal** | v0.2 | XPath-inspired containment tree queries. Synergy with geometry selectors. See §6.2. |
| **Geometry selector strengthening** | v0.2 | `@face(nearest point)` and other representation-independent selectors to supplement name-based selectors. Addresses the persistent naming problem for parametric topology changes. |
| **`Result<T>` or `fallback` expressions** | v0.2 | User-catchable errors for geometric operations with fallback. Only if v0.1 error model (§4.1) proves insufficient. |
| **Associated `fn` in traits** | v0.2+ | Trait methods with per-type implementations. Only if trait dispatch without method bodies proves too limiting. |
| **Data-carrying enums** | v0.2+ | Rust-style algebraic data types. Only if C-style enums prove insufficient. |
| **Tolerance stack-up analysis** | v0.2 | RSS, worst-case, Monte Carlo methods. Requires assembly graph + statistical computation. Framework in `std.tolerancing`; methods in `std.analysis`. |
| **Keyed collection identity** | v0.2 | Trait-based element identity for collections (angular, spatial, user-defined). Replaces positional indexing as default. |

---

## 8. Declaration kind summary (post-review)

The language has four entity types and three non-entity declaration kinds:

| Declaration kind | Entity? | Has identity? | Has determinacy? | Eval graph presence? |
|---|---|---|---|---|
| `structure` | Yes | Yes | Yes | ValueCells, RealizationNodes, etc. |
| `occurrence` | Yes | Yes | Yes | ValueCells, RealizationNodes, etc. |
| `constraint` | Yes | Yes | Yes | ConstraintNodes |
| `field` | Yes | Yes | Yes | ValueCells, ComputeNodes |
| `fn` | No | No | No | Inlined into dependent nodes |
| `trait` | No | No | No | None — compile-time only |
| `purpose` | No — but has AST identity | Yes (for activation/diagnostics) | No | Contributes constraints/outputs when activated |

Additionally: `enum` and `unit` are language-level keywords for declarations that don't fit the entity/non-entity split — they are type-level constructs.

---

## 9. Keyword set (post-review)

### Added
- `trait` (formalised — was assumed but not in grammar)
- `purpose` (formalised — was described as sugar)
- `meta` (from deferred syntax completion)
- `match` (from deferred syntax items)
- `fn` (from deferred syntax items)
- `enum` (from standard library boundary)
- `unit` (from standard library boundary)

### Removed
- `derived` (replaced by `let`)
- `require` (replaced by `constraint` + determinacy predicates)
- `dimension` (replaced by `let` for type aliases)

### Unchanged
- `structure`, `occurrence`, `constraint`, `field`
- `param`, `let`, `port`, `sub`, `pub`
- `where`, `connect`, `chain`
- `module`, `import`
- `self`
- `undef`, `auto`
- `true`, `false`, `some`, `none`
- `and`, `or`, `not`, `implies`
- `forall`, `exists`
- `if`, `then`, `else`
- `in`, `out`
- `minimize`, `maximize`
- `set` (prefix for set literals)

---

*Document generated from comprehensive design review session. Intended as a delta to be applied during specification synthesis. All cross-references point to the specific sections in existing design decision documents that are affected by each resolution.*
