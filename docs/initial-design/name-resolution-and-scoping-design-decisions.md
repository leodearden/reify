# Name Resolution and Scoping: Design Decisions

**Status:** Foundation complete — ready for module system design and deferred syntax completion  
**Version:** 0.1 — First crystallization from name resolution and scoping design sessions  
**Builds on:** `ontology-design-decisions.md` v0.1, `type-system-design-decisions.md` v0.1, `syntax-design-decisions.md` v0.1, `constraint-system-design-decisions.md` v0.1, `evaluation-graph-design-decisions.md` v0.1, `evaluation-graph-completion-design-decisions.md` v0.2

---

## 1. Design approach

Name resolution and scoping are foundational to the language — they determine how declarations are found, how entities refer to each other, and how the design hierarchy maps to lexical structure. The design prioritises uniformity, minimal boilerplate, and compositional power.

**Core principle:** A single, uniform scoping model covers all entity types (structures, occurrences, constraints, fields), all nesting depths, and both definition and specialisation contexts. No special cases per entity type.

---

## 2. Lexical scoping with declarative order-independence

### 2.1 Lexical scoping

All name resolution is lexical. A name reference resolves to the lexically enclosing declaration. There is no dynamic scoping — the meaning of a name is determined by its position in the source text, not by runtime context.

**Rationale:** Dynamic scoping would make reasoning about constraints impossible. When a constraint references `diameter`, the binding must be statically determinable by both humans and tools. This is non-negotiable for a language where constraints are first-class and compose across scope boundaries.

### 2.2 Order-independence

All declarations within a body are mutually visible regardless of textual order. A constraint may reference a parameter declared below it; a structure may contain occurrences that reference sibling occurrences declared later in the body.

**Rationale:** The language is declarative, not imperative. Requiring topological ordering of declarations would impose an artificial sequencing constraint on what is fundamentally a set of simultaneous declarations. It would also be hostile to both human editing and LLM generation.

### 2.3 Function parameter default scope

Default expressions on function parameters (`fn f(x : Real = expr)`) are compiled in a neutral scope containing only module-level names. They cannot reference sibling parameters and cannot recurse into the enclosing function.

**Rationale:** Keeps defaults pure-by-construction and order-independent — neither parameter-declaration order nor recursive self-reference can affect a default's compiled meaning. The same logic that gives order-independence to sibling parameter declarations (§2.2) would otherwise be in tension with sibling-default references.

**Locked in by:** Implementation at `compile_function` in `crates/reify-compiler/src/functions.rs` and the regression test `fn_param_default_sibling_param_ref_errors` in `crates/reify-compiler/tests/fn_param_default_consumption_tests.rs`.

---

## 3. Scope visibility rules

### 3.1 Downward visibility: parent scopes see children via dot notation

A parent scope accesses a child's declarations via dot notation: `occurrence_name.param_name`. Dot chains compose: `assembly.bracket.hole.diameter`.

**Visibility boundary:** Only parameters and named occurrences are accessible from outside a scope. Local bindings (intermediate computations, internal constraints) are private.

**Rationale:** Parameters are the explicit interface of a structure — they are what the designer intends to expose for configuration and constraint. Named occurrences are sub-components — they represent the structural hierarchy that engineers expect to navigate. Local bindings are implementation details. This boundary matches engineering intuition: you expect to see a bracket's mounting holes (named occurrences) and its material thickness (parameter), but not its internal scratch calculations.

**Upgrade path:** If experience reveals cases where finer-grained access control is needed, a `pub` keyword can be added to expose specific local bindings. The default (params and occurrences public, locals private) remains unchanged.

### 3.2 Upward visibility: child scopes see parent scopes

A lexical scope has access to the bound names in the parent lexical scope, transitively. A constraint inside a nested occurrence can reference parameters of the enclosing structure, or of the structure enclosing that, and so on up the lexical chain.

**Rationale:** This eliminates the boilerplate of threading common parameters downward through the hierarchy. Engineering models frequently need cross-level references — a constraint on a sub-component's clearance relative to an enclosing housing dimension, for example. Requiring explicit parameter passing for every such reference would be verbose and fragile.

**No implicit upward leaking:** A child's declarations do not enter the parent namespace. Accessing a child's declarations requires explicit dot notation from the parent. Visibility is asymmetric: children see parents implicitly; parents see children explicitly.

### 3.3 Deep dot chains

Unlimited dot-chain depth is permitted: `assembly.subassembly.bracket.rib_pattern.rib[3].fillet.radius`.

**Warning policy:** The compiler warns on deep chains (threshold configurable, suggested default: 3–4 levels) and suggests using ports or explicit delegation as alternatives. Deep chains create tight coupling to internal structural hierarchy, which is fragile under refactoring.

**Rationale for allowing rather than forbidding:** Engineers think in terms of assembly trees and expect to reach into them. Forbidding deep access forces premature abstraction via ports, adding boilerplate for exploratory design. The warning provides guidance without preventing legitimate use.

---

## 4. Shadowing

### 4.1 Policy: warn, not forbid

When a declaration in a child scope uses the same name as a declaration visible from a parent scope, the compiler emits a warning. The shadowing is permitted — the child's declaration takes precedence within the child scope, and the parent's declaration remains accessible via dot notation from other scopes.

### 4.2 Rationale

With explicit name declarations (not implicit binding-on-first-assignment), shadowing requires a deliberate declaration with a keyword (`param`, `occurrence`, etc.). Accidental shadowing is unlikely. However, it can harm readability — a human reader may be uncertain which `diameter` a reference in a nested scope refers to. The warning surfaces this for review.

### 4.3 The undetectable case

The more dangerous error — using a parent's name *intending* a local one but forgetting to declare it — is semantically indistinguishable from correct parent-scope usage. The compiler cannot detect this. Good naming conventions and short scopes are the practical mitigation.

---

## 5. Self-reference

### 5.1 The `self` keyword

Within a structure or occurrence body, `self` refers to the entity being defined or specialised. `self.param_name` is equivalent to `param_name` for locally declared names, but `self` is required in contexts where the entity itself (rather than one of its members) is the referent — for example, passing the entity to a constraint: `constraint all_geometric_params_determined(self)`.

### 5.2 Rationale

Using the entity's declared name for self-reference creates a fragile coupling: renaming the entity requires finding and updating internal self-references. `self` is unambiguous, conventional, and refactoring-safe.

---

## 6. Occurrence instance scoping

### 6.1 Specialisation scopes

When an occurrence is instantiated within a parent body, its body is a **specialisation scope**. This scope:

- **Sees the parent scope** (and transitively, all ancestor scopes) — enabling constraints that reference sibling occurrences, enclosing parameters, and other contextual declarations.
- **Can set parameters and add constraints** on the occurrence instance.
- **Does not modify the underlying definition** — the structure or occurrence type's original definition remains lexically closed over its own definition site.

```
structure Assembly {
    param clearance: Length

    occurrence motor: ElectricMotor {
        shaft_diameter = 8mm
    }

    occurrence coupling: ShaftCoupling {
        bore = motor.shaft_diameter    // cross-reference to sibling via parent scope
        constraint bore_clearance: bore - motor.shaft_diameter >= clearance  // references parent param
    }
}
```

### 6.2 Rationale

This separation — definitions are self-contained, instances are contextual — matches engineering practice. A motor's definition doesn't know about the assembly it will be used in. But when placed in an assembly, constraints naturally arise between it and its neighbours. The specialisation scope is where those contextual constraints live.

---

## 7. Trait member scoping

### 7.1 Trait members merge into implementing scope

When a structure implements a trait, the trait's declared members (parameters, constraints, occurrences) become part of the structure's body. They are accessed without qualification — as if declared directly in the structure.

### 7.2 Trait conflict resolution

- **Same name, same type:** No conflict. The single declaration satisfies both traits.
- **Same name, different type:** Error. The implementing structure cannot satisfy both. This is detected at definition time.
- **Same name, same type, different constraints:** No conflict. Both traits' constraints apply (conjunction). If the constraints are contradictory, this is a normal constraint violation detected at evaluation time — not a trait conflict.

### 7.3 Exact type matching

For v0.1, trait member compatibility requires **exact type match**, not subtype compatibility. If trait A declares `param stiffness: Force / Length` and trait B declares `param stiffness: Pressure`, these are different types and the conflict is an error, even if they are dimensionally related. Subtype-compatible merging introduces subtle edge cases that are not justified until real usage patterns clarify the need.

---

## 8. Recursive structures

### 8.1 Eager unfolding

Recursive structure definitions are permitted. Structural unfolding is eager — once the parameters controlling recursion depth are determined, the full instance tree is materialised in the evaluation graph.

```
structure TreeBracket {
    param depth: Natural
    param thickness: auto
    occurrence sub: TreeBracket? where depth > 0 {
        depth = self.depth - 1
    }
    constraint thickness >= sub.thickness * 1.2 where depth > 0
}
```

### 8.2 Termination

A recursive structure definition must have a termination condition — an optional type, a predicate guard, or a variant type that provides a base case. A recursive definition with no reachable termination condition is a static error.

`undef` is not a valid termination mechanism. `undef` means "not yet decided," not "structurally absent." Structural presence or absence must be deterministic once the controlling parameters are determined.

### 8.3 Unfolding preconditions

Unfolding requires the recursion-controlling parameters to be determined. A `TreeBracket` with `depth = auto` cannot be unfolded — the graph structure depends on the value. The parameter must be resolved (via constraints, explicit assignment, or resolution of `auto` by the constraint solver) before structural unfolding proceeds.

If the recursion-controlling parameter is itself `auto`, it must be resolvable from constraints that don't depend on the recursive structure's internal topology. This is a natural consequence — you can't determine the shape of something whose shape depends on the answer.

### 8.4 Resolution order

Resolution does not cross recursive instance boundaries. Each instance resolves independently based on its determined inputs. The evaluation graph's pull-based, demand-driven evaluation naturally resolves leaf instances (deepest recursion, most determined) before their parents. No recursion-specific scheduling logic is needed.

**Rationale:** This is consistent with the occurrence-as-specialisation model — each occurrence is resolved in its own context with its inputs treated as given. The normal dataflow dependency graph produces the correct resolution order.

### 8.5 Structural changes on parameter change

If a recursion-controlling parameter changes (e.g., `depth` changes from 3 to 5), the instance tree must be re-unfolded. New instances appear; if depth decreases, instances are removed (and their warm state is available for future reuse via the warm-state pool). This is a specific case of the general graph structural changes problem (evaluation-graph-design-decisions §12.5) but is well-structured: additions and removals follow a known recursive pattern.

---

## 9. Guarded declarations: structural vs. constraining `where`

### 9.1 The distinction

A `where` clause on different declaration types has different semantics:

- **`where` on a constraint:** Controls whether the constraint is *structurally present*. When the guard is false, the constraint does not exist in the evaluation graph. It is not evaluated, does not contribute to the feasible region, and has no associated nodes.
- **`where` on a structure, occurrence, or field:** Controls whether the entity is *structurally present*. When the guard is false, the entity does not exist — it is not instantiated, has no nodes in the evaluation graph, and cannot be referenced.

All entity types follow the same rule: a `where` guard controls structural presence. There is no "vacuously satisfied" special case for constraints. This pushes constraint guard-flips into the same structural-change mechanism as conditional occurrences and structures, but that mechanism is required regardless — conditional occurrences demand it — and a uniform rule is preferable to a special case that exists only to avoid a problem that must be solved anyway.

### 9.2 Design status

This distinction is identified but the surface syntax is not yet resolved. Both uses currently share the `where` keyword. Whether this overloading is acceptable (the context — occurrence vs. constraint — disambiguates) or whether distinct syntax is needed is deferred to the syntax completion phase.

**Consideration:** The structural `where` is load-bearing for recursive termination (§8.2), conditional sub-structure presence, and conditional constraint activation. Its semantics are uniform across all entity types: a guarded-out entity is absent, not `undef` or vacuously satisfied. Guard-flips on any entity type — including constraints — are structural changes, handled by the graph structural change mechanism (evaluation-graph-design-decisions §12.5).

---

## 10. `auto` in recursive contexts

### 10.1 Candidate evaluation errors

When the constraint solver explores candidate values for `auto` parameters:

- **Candidate violates a constraint:** Normal solver operation — the candidate is outside the feasible region. The solver prunes it silently and continues. No warning.
- **All candidates exhausted / no feasible region found:** Resolution failure. Propagates as `indeterminate` with a diagnostic. The determinacy stack trace mechanism provides the explanation.
- **Candidate triggers a structural or type error in the definition:** Propagates as an error. This indicates the type definition is broken regardless of candidate value — it is a definition error, not a bad candidate.

### 10.2 Cross-instance resolution boundaries

Resolution does not cross recursive instance boundaries. Each instance resolves its own `auto` parameters independently. The parent resolves first (or rather, the evaluation graph resolves in dependency order — typically leaves first), and resolved values feed into dependent instances as determined inputs.

This avoids fixpoint computation across the recursive structure. The upgrade path to full cross-instance resolution exists if real use cases demand it, but it is not justified for v0.1.

---

## 11. Open questions for subsequent design phases

### 11.1 Module system (next phase)

- File-level scoping and the relationship between files and namespaces
- Import/export syntax and semantics
- Visibility across module boundaries (does the params-and-occurrences-public rule apply at module boundaries, or is a separate `pub` mechanism needed?)
- Qualified vs. unqualified name access across modules

### 11.2 Syntax completion

- Optional types for recursive termination (`TreeBracket?` syntax and semantics)
- Surface syntax for guarded occurrences — whether `where` overloading is acceptable or distinct syntax is needed
- Pattern matching (needed for variant types and potentially for recursive base cases)
- Port mapping syntax
- Frame projection operator

### 11.3 Standard library implications

- Which built-in traits are auto-imported into every scope?
- Whether a prelude/implicit import mechanism exists and what it contains

---

## 12. Summary of key decisions

| Decision | Choice | Rationale |
|---|---|---|
| Scoping model | Lexical, order-independent | Declarative language; static name resolution essential for constraint reasoning |
| Upward visibility | Child sees parent, transitively | Eliminates parameter-threading boilerplate; matches engineering cross-level references |
| Downward visibility | Dot notation; params and named occurrences public, locals private | Clear interface boundary matching engineering intuition; clean upgrade path via `pub` |
| Deep dot chains | Allowed with compiler warning | Engineers expect assembly-tree navigation; warning discourages fragile coupling |
| Shadowing | Warned, not forbidden | Explicit declarations make accidental shadowing unlikely; warning aids readability |
| Self-reference | `self` keyword | Refactoring-safe, unambiguous, conventional |
| Occurrence instances | Specialisation scopes: see parent, can constrain, don't modify definition | Definitions are self-contained; contextual constraints live at the instantiation site |
| Trait conflicts | Same name + same type = no conflict; constraints compose conjunctively; different types = error | Simple, predictable; avoids arbitrary priority rules between traits |
| Trait type matching | Exact match for v0.1 | Subtype merging defers complexity until usage patterns justify it |
| Recursive structures | Eager unfolding; termination required; unfolding needs determined control params | Natural fit with eval graph's demand-driven resolution; `undef` ≠ absent |
| Resolution boundaries | No cross-instance resolution in recursive structures | Each instance resolves independently; dependency graph handles ordering naturally |
| `auto` candidate errors | Constraint violation = prune silently; exhaustion = `indeterminate`; type/structural error = propagate | Solver operation is silent; definition errors surface to designer |
| Function parameter defaults | Compiled in a neutral scope — only module-level names visible, no sibling params, no recursion | Defaults stay pure-by-construction and order-independent |

---

*Document generated from name resolution and scoping design sessions. Intended as a living specification to be refined through module system design and syntax completion.*
