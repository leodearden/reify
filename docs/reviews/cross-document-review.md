# Cross-Document Review: Reify Language Specification & Implementation Architecture

## 1. Cross-Document Consistency

### 1.1 Terminology Alignment

The two documents are remarkably well aligned on terminology. Both consistently use the same names for core concepts: `structure`, `occurrence`, `constraint`, `field`, `trait`, `fn`, `purpose`, `ValueCell`, `DeterminacyState`, `Currency`, `auto`, `undef`. This is clearly the product of disciplined synthesis from shared design decisions.

**Minor inconsistencies found:**

- **"Realization" vs "Realisation"**: The language spec uses American spelling in some places (`realization` at line 1476: "A realization event with `EventKind::error` is emitted") while the architecture document consistently uses British spelling (`realisation`, `RealisationEvent`, `RealisationNode`). The architecture is internally consistent with British spelling; the language spec mixes. This should be normalised. The node type in the architecture is `RealizationNode` (American spelling at line 99: "RealizationNode"), while events use `RealisationEvent` (British). This inconsistency within the architecture document itself is more concerning -- it affects API surface.

- **`Solid` boundedness**: Both documents agree that `Solid` no longer implies `Bounded`. The language spec (line 366) says "Closed region of 3D space (closed, not necessarily bounded)" while the architecture (line 710) says "Closed bounded region of 3D space." This is a direct contradiction in the architecture document's geometry type table, which still includes "bounded" in the definition despite the subsequent note (line 719) correctly stating "Solid no longer implies Bounded."

- **`DeterminacyState` values**: The architecture (line 59) lists `undef | constrained | auto | determined`. The language spec (section 9.1) describes the same four states but uses the phrase "Constrained but not determined." These map cleanly but the architecture uses short identifiers while the language spec uses longer descriptions. The architecture's enum is the canonical form.

### 1.2 Currency Enum and Error Model

The Currency enum in the architecture (4 variants: `Final`, `Intermediate`, `Pending`, `Failed`) matches exactly between the architecture's section 7.1/9.2 and the language spec's section 9.6. Both documents agree on the error handling model: no `Result<T, E>`, no `try`/`catch`, failures are graph-level events that produce `Failed` currency. This is one of the most consistent cross-document elements.

### 1.3 Node Taxonomy vs Language Constructs

The architecture defines 6 node types. Here is the mapping:

| Language Construct | Evaluation Graph Node(s) | Coverage |
|---|---|---|
| `param` declaration | ValueCell | Complete |
| `let` binding | ValueCell | Complete |
| `constraint` (inline & named) | ConstraintNode | Complete |
| `auto` parameters | ResolutionNode | Complete |
| Geometry (opaque types, operations) | RealizationNode | Complete |
| FEA, CFD, export, analysis | ComputeNode | Complete |
| `where` guards, collection sizes | SchemaNode | Complete |
| `field` entity | ValueCell + ComputeNode | Partially implicit |
| `fn` declaration | None (inlined) | Correct per design |
| `trait` declaration | None (compile-time only) | Correct per design |
| `purpose` declaration | Constraints/outputs when active | Complete |
| `connect` statement | Unclear | **Gap** |
| `chain` statement | Unclear | **Gap** |
| `match` block (declaration-level) | Desugars to `where` guards -> SchemaNode | Implicit |
| `meta` block | None (opaque to eval graph) | Correct per design |
| `enum` declaration | Compile-time only | Correct |
| `module`/`import` | Compile-time only | Correct |

**Identified gaps:**

1. **`connect` statements have no explicit evaluation graph mapping.** The language spec describes `connect` as creating a connector structure, constraints, frame alignment constraints, and a topology edge. The architecture never describes how these decomposed artifacts enter the evaluation graph. It is implicit (connector becomes a structure with its own ValueCells, constraints become ConstraintNodes, etc.) but should be stated explicitly.

2. **`chain` has no implementation story.** It is described as sugar for `connect`, so it presumably desugars before the evaluation graph is built, but this is not stated.

3. **`field` entities** are listed in the architecture's declaration kinds table (line 35) as having ValueCells and ComputeNodes, but there is no dedicated section explaining how field source kinds (`analytical`, `sampled`, `composed`, `imported`) map to nodes. Analytical fields could be ValueCells containing closures, sampled fields could be ComputeNodes for interpolation setup, imported fields could require I/O. This mapping is under-specified.

### 1.4 Data Types and Grammar Match

The grammar in the language spec (section 13) is comprehensive and internally consistent. The architecture does not reproduce the grammar (correctly -- it should not), but its pseudocode uses type expressions and constructs consistent with the grammar.

One tension: the architecture's SchemaFragment (line 130-134) uses types like `Set<NodeDeclaration>`, `Map<OccurrenceId, SchemaFragmentRef>`, and `ContentHash` that are implementation types, not language types. This is appropriate -- these are internal data structures -- but the boundary between "language-level types the user sees" and "implementation types the runtime uses" could be more explicitly drawn.

### 1.5 Declaration Kinds Table

The architecture's declaration kinds table (section 1.2) lists 7 kinds. The language spec's keyword list and grammar define the same 7. There is a subtle mismatch: the architecture's table shows `purpose` as having "AST identity" but "No" for the "Entity?" column. The language spec (section 4.4) calls purposes "named, parameterised declaration kinds with AST identity." Both agree purposes are not entities, but the architecture table would benefit from a footnote explaining "AST identity" vs "entity identity."

---

## 2. Interface Completeness

### 2.1 Language Construct to Graph Node Mapping

For most constructs, the mapping is clear. The strongest areas:

- **`param` -> ValueCell**: Crystal clear. The architecture (section 2.1) specifies per-parameter granularity, determinacy state tracking, and the 10,000-parameter scaling example.
- **`constraint` -> ConstraintNode**: Well-specified with `Satisfaction` enum, structured diagnostics, and quantifier handling.
- **`auto` -> ResolutionNode**: The cycle resolution (apparent cycle is internal convergence loop) is well-explained. The trial snapshot mechanism provides clean isolation.
- **`where` guard -> SchemaNode**: The two-pass elaboration (partial -> resolution -> full) elegantly handles the `auto`-feeding-structure case.

**Weakest areas:**

- **Ports and connections**: The language spec describes ports as "typed scopes with members" and connections as creating connector structures, constraints, and frame alignment. The architecture never addresses how port compatibility checking works at the graph level, how frame alignment constraints are generated and evaluated, or how connector ownership is resolved. This is a significant gap -- connections are arguably the most important user-facing mechanism for assembly composition.

- **Ad-hoc ports (`@` operator)**: The language spec defines `@face`, `@region`, `@point`, `@edge`, `@body` selectors. The architecture mentions the persistent naming problem as an open question (item 10 in the open questions table) but provides no implementation guidance for how ad-hoc ports interact with the evaluation graph, geometry kernels, or caching.

- **Implicit deref for single-payload ports**: The language spec (line 777) defines this convenience feature but the architecture says nothing about how implicit deref is resolved at the graph level. Is it a compile-time desugar? Does it produce different ValueCell references? This is a compile-time concern but deserves a note.

### 2.2 Runtime Behavior Completeness

| Runtime Behavior | Language Semantics | Implementation Mechanism | Complete? |
|---|---|---|---|
| `undef` propagation | "Propagates through dependent computations" | ValueCell with `undef` state; downstream nodes evaluate to `indeterminate` | Mostly -- edge cases around partial propagation ("may be contained/swallowed where downstream computation doesn't depend") need more implementation detail |
| `auto` resolution | Strict vs free semantics well-defined | ResolutionNode with trial snapshots, scope-level decomposition | Yes |
| `where` guard flip | Structural presence/absence | SchemaNode re-elaboration, node creation/removal, warm-state donation | Yes |
| Constraint violation | "Evaluation continues" | `violated` satisfaction, priority reduction for downstream | Yes |
| Computation failure | `Failed` currency, downstream `Pending` | Graph-level `Failed` variant, diagnostic chain | Yes |
| Recursive structure unfolding | Eager once depth is determined | SchemaNode evaluates recursive depth parameter; static cycle detection | Yes, but no detail on recursion depth limits |
| Collection size change | Schema re-elaboration | Same as guard flip | Yes |
| Purpose activation/deactivation | Same as structural change | Same as guard flip | Yes |

**Semantic questions the language spec leaves unanswered:**

1. **What happens when `undef` propagates through a `match` expression?** If the discriminant is `undef`, does every branch evaluate? Does the match become `undef`? The language spec says `undef` propagates "to the extent that the result relevantly depends on the undefined input" but does not address `match` specifically. The architecture should specify whether the ConstraintNode or ValueCell evaluates all branches or short-circuits.

2. **Order of constraint checking during `auto` resolution**: The language spec says bottom-up resolution is the default. The architecture describes the ResolutionNode's convergence loop but does not specify what happens when a parent scope's objective conflicts with a child scope's resolved result. The language spec acknowledges this is "an approximation when there is coupling" but the architecture does not describe how coupling is detected or what diagnostic is surfaced.

3. **`auto` for type parameters**: The language spec (section 3.9) says `auto` is valid for type parameters. The architecture never addresses how type parameter resolution works in the evaluation graph. Type parameters are resolved at definition time (compile time per the language spec), but `auto` type parameters could require constraint-based selection, which would need graph-level representation.

### 2.3 Semantic Questions Neither Document Answers

1. **What is the semantics of `forall v in vents: constraint ...` when `vents` is empty?** Vacuous truth? The language spec does not state this explicitly. For `forall`, empty-collection vacuous truth is standard, but engineers may find it surprising.

2. **Chained comparison with `undef`**: `2mm < undef < 10mm` -- does this propagate `undef` or become `indeterminate`? The language spec defines chained comparisons as desugaring to `and`, and the architecture defines `indeterminate` satisfaction for `undef` inputs, so the answer is probably `indeterminate`, but this should be explicit.

3. **`Int` promotion to `Real` in unit expressions**: What happens with `5mm + 3`? The `3` is `Int` (dimensionless). `Int` promotes to `Real`, but `Real` is dimensionless. Is `5mm + 3.0` a dimension error? The language spec says "Bare numbers are dimensionless" but does not explicitly address mixed arithmetic.

---

## 3. Overall System Coherence

### 3.1 Mental Model

The system has a strong, coherent mental model built on four pillars:

1. **Declarative specification**: Engineers describe what they want, not how to compute it.
2. **Progressive refinement**: Designs move from `undef` through `constrained` to `determined`.
3. **Demand-driven evaluation**: Nothing is computed until needed.
4. **Source-as-canonical**: No privileged geometric representation.

These four principles are consistent and mutually reinforcing. The architecture implements them faithfully. The evaluation graph as a DAG of typed nodes is a clean abstraction that maps naturally to the declarative language semantics.

### 3.2 Design Principle Conflicts

**Conflict 1: Regularity vs domain expressiveness.** The language spec prioritises regularity ("every entity type follows the same declaration shape"), but the four entity kinds (structure, occurrence, constraint, field) have quite different semantics and member sets. Fields have `source = analytical { ... }` syntax that no other entity uses. This is acknowledged implicitly but creates a tension: the grammar is uniform but the semantics are not.

**Conflict 2: Concision vs explicitness.** `connect` with implicit port matching, implicit deref for single-payload ports, and `chain` sugar all favour concision. But the explicitness principle says "structure visible in text." When a `connect` statement creates a connector structure, constraints, and frame alignment automatically, significant structure is hidden from the text. The language spec does not resolve this tension clearly.

**Conflict 3: Immutability vs interactive performance.** The architecture commits to immutable snapshots but acknowledges mutable warm-start state, mutable reverse dependency indices, and mutable scheduling infrastructure. The mutability audit (section at the end of the architecture doc) is excellent and clearly delineates what is load-bearing immutable vs mutable-but-safe. This conflict is well-resolved.

**Conflict 4: Bottom-up resolution vs global optimality.** The language spec acknowledges bottom-up resolution is approximate when scopes are coupled but provides no mechanism for the user to request global optimisation. This is a real design tension: bottom-up is tractable but sub-optimal; global is intractable but correct. The system provides no escape hatch beyond "broaden the optimisation scope," which the designer may not know how to do.

### 3.3 Complexity Budget

The complexity budget is generally well-allocated:

- **Over-invested**: The standard library specification in the language spec is extremely detailed (sections 11.1-11.14, roughly 40% of the document). While thorough, many of these are API surface decisions that could change without affecting the core language design. The level of detail on material trait hierarchies, port types, and tolerancing structures is premature for a v0.1 spec -- it couples the language design review to domain modelling decisions.

- **Under-invested**: The `connect`/`chain` semantics and their implementation get surprisingly little attention relative to their importance. For an engineering design language, assembly composition through connections is likely the most-used feature. The language spec devotes about 70 lines; the architecture devotes zero dedicated lines.

- **Well-balanced**: The evaluation graph, caching, warm starting, and scheduling model receive appropriate depth. The constraint system overview is well-sized. The geometry engine architecture correctly defers kernel implementation details while establishing the right abstractions.

---

## 4. Quality Assessment -- Will This Work?

### 4.1 User Experience

**Strengths:**
- The determinacy spectrum (`undef` -> `constrained` -> `auto` -> `determined`) is an elegant concept that maps well to the engineering design process. Engineers naturally work from underdetermined sketches to fully specified parts.
- Quantity literals with units (`5mm`, `3.2kN`) are excellent for readability and error prevention.
- Chained comparisons (`2mm < thickness < 10mm`) are natural for engineering ranges.
- The `where` guard for conditional presence is intuitive for variant-heavy designs.

**Concerns:**
- **43 keywords is a lot.** For comparison, Rust has 39 (plus 14 reserved). The cognitive load is significant, especially for occasional users.
- **The four entity kinds (structure, occurrence, constraint, field) require the user to make a classification decision upfront.** In practice, the boundary between a structure and an occurrence is fuzzy. Is a "welded joint" a structure or an occurrence? The spec says occurrences "transform structures," but this distinction may not be intuitive.
- **`sub` keyword for instantiation is non-standard.** Most engineers will think "component" or "part." The choice of `sub` is concise but requires explanation.
- **No REPL or interactive evaluation story.** For an interactive design tool, the ability to evaluate expressions incrementally would be extremely valuable for learning and debugging.

### 4.2 LLM Co-authoring

**Strengths:**
- Regular grammar (aiming for LL(1)) is good for LLM generation.
- Curly-brace blocks with explicit keywords avoid the indentation sensitivity that trips LLMs.
- Mandatory type annotations on parameters reduce ambiguity.
- Unit syntax (`5mm` with no space) is a clear, unambiguous token.

**Concerns:**
- **`let` overloading for both value bindings and type aliases.** An LLM generating code must determine from context whether `let Force = Mass * Length / Time^2` is a type alias or a value computation. The PascalCase/snake_case convention helps, but LLMs will get this wrong sometimes.
- **`connect` statement complexity.** The full `connect` syntax with optional connector type, optional port mapping, optional parameter block, and optional `@` selectors creates a combinatorial space that LLMs will struggle to generate correctly for complex cases.
- **Qualified trait access syntax** (`bracket.(Rigid::max_temperature)`) has unusual parenthesisation that LLMs may not reproduce reliably.
- **Unit expression parsing.** `5kg*m/s^2` requires understanding operator precedence within unit expressions. LLMs will sometimes produce `5kg*m/s2` or `5(kg*m/s^2)` when they mean different things.

### 4.3 Technical Feasibility

**Feasible with effort:**
- The evaluation graph with immutable snapshots and HAMT structural sharing is well-understood technology (Salsa, Adapton, Incremental.jl). Building it in Rust with the described semantics is solid engineering.
- Content-hash caching with warm starting is proven (build systems like Buck2, Bazel).
- Multi-kernel geometry dispatch is ambitious but the architecture correctly identifies the right kernels (OCCT, Manifold, SolveSpace).

**Research problems disguised as engineering:**
1. **Constraint orchestration across domains.** Section 11.1-11.7 of the architecture describes the orchestrator pattern for constraint solving, but the actual algorithm for decomposing cross-domain constraints, dispatching to sub-solvers, and managing feedback between coupled sub-solvers is an open research problem. The constraint system open questions (C-10.1 through C-10.8) are honest about this, but the amount of open research is substantial.

2. **`auto` resolution with strict uniqueness.** Strict `auto` requires the resolved value to be "uniquely determined or uniquely optimal." Proving uniqueness in a nonlinear constraint system is generally undecidable. The spec says "with the global default objective (centrality/robustness), strict `auto` is well-defined almost everywhere" but this is an assertion, not a proof. In practice, this will require significant heuristic engineering.

3. **Tolerance budget allocation across representation chains.** The architecture lists this as open question #1 and correctly identifies it as a v0.1 priority, but this is a genuine research problem. Allocating error budgets across B-rep -> mesh -> SDF -> voxel chains while maintaining bidirectional guarantees has no known general solution.

4. **Persistent naming for geometry selectors.** The architecture lists this as open question #10. This is one of the most fundamental unsolved problems in parametric CAD. Without it, `@face(name)`, `edges(solid)`, and similar selectors will break across geometry regeneration. This undermines the stability of any constraint or connection referencing geometric features.

### 4.4 Performance

**Likely fine:**
- Interactive editing of scalar parameters should be responsive. The two-cone scheduling model with P0 priority for property editor reads is well-designed.
- Content-hash caching with early cutoff will prevent cascading recomputation in the common case.
- Warm starting for geometry kernels will keep interactive response times acceptable for moderate designs.

**Likely problematic:**
- **SchemaNode re-elaboration on structural changes.** Toggling a `where` guard on a large assembly triggers full schema re-elaboration for the affected scope and all ancestors. The early cutoff on `structure_version` hash helps, but complex assemblies with many conditional sub-structures could see latency spikes.
- **`forall` over large collections.** A `forall` quantifier over 1,000 elements creates one ConstraintNode with edges to all 1,000 elements' ValueCells. Changing any element dirties the constraint. The architecture does not discuss batching or incremental quantifier evaluation.
- **ResolutionNode convergence time.** The trial snapshot mechanism creates full snapshots for each solver iteration. For a scope with many `auto` parameters and tight constraints, the solver could require dozens of iterations, each creating a new snapshot and evaluating constraints and realisations. The HAMT sharing mitigates memory, but the evaluation cost could be significant.

### 4.5 Extensibility

**Well-designed extension points:**
- **Traits**: Clean compositional mechanism for extending structure/port/material capabilities.
- **`@optimised` hook**: Elegant bridge between language-level definitions and optimised runtime implementations.
- **Domain libraries**: The standard library tree is well-structured for community extension.
- **Multi-kernel dispatch**: New geometry kernels can be added by registering capabilities.

**Poorly designed extension points:**
- **No user-defined annotations.** The annotation system (`@optimised`, `@deprecated`, `@test`) is closed. Users cannot define domain-specific annotations. This limits library authors' ability to provide rich tooling integration.
- **No plugin/extension system for the constraint orchestrator.** The architecture describes how to register geometry kernels but not how to register custom constraint solvers for domain-specific problems.

### 4.6 Maintainability

**Strengths:**
- The mutability audit at the end of the architecture document is excellent practice. It clearly categorises what is immutable, what is mutable-but-encapsulated, and what is mutable-outside-the-model.
- The separation of state (cached results) and history (realisation events) is clean and will prevent the "everything is an event" anti-pattern.
- The open questions appendix is honest and well-prioritised.

**Risks:**
- **The warm-start protocol** is described as a pure interface wrapping mutable state. Maintaining the semantic invariant (absent warm state -> cold compute -> identical result) requires discipline as more kernel integrations are added. Each kernel integration is a new place where this invariant could be violated.
- **The SchemaNode's dual responsibility** (topology production AND schema composition via containment tree) could become a god-object as the system scales. The architecture's open question #6 ("whether SchemaNode is a 6th node type or a specialised ComputeNode") suggests this concern is already present.

---

## 5. What's Missing from the Overall Design

### 5.1 IDE/Tooling Integration

Neither document specifies the interface between the evaluation graph and an IDE/editor. The architecture mentions "the property editor," "the constraint panel," and "the 3D viewport" but never defines the API through which tooling communicates with the runtime. For a system where interactive editing is the primary workflow, this is a significant omission.

Needed:
- Language server protocol (LSP) integration or equivalent
- How the demand registry is populated from editor state
- How syntax errors in partially-edited source interact with the evaluation graph
- How auto-complete works for port names, parameter names, unit names, trait members

### 5.2 Debugging and Inspection

The architecture mentions "determinacy stack traces" (section 8.4) as on-demand backward walks and "dispatch plan inspectability" for kernel dispatch. But there is no systematic debugging story:
- How does a user inspect the current value of a deeply nested parameter?
- How does a user understand why a constraint is violated?
- How does a user trace why `auto` resolved to a particular value?
- How does a user compare two snapshots?

These are critical for usability and neither document addresses them beyond mentioning that the data is available.

### 5.3 Version Control and Collaboration

Neither document addresses:
- How `.ri` files interact with git or other VCS
- How merge conflicts in `.ri` files are resolved (the `Merge` provenance variant exists but no conflict resolution strategy is described)
- Multi-user concurrent editing
- Design branching (exploring alternatives)

The `Merge` snapshot provenance variant suggests some thought has been given, but the `ConflictResolution` type is undefined.

### 5.4 Testing

The language spec mentions `@test` as an annotation (section 12.1) but provides no semantics:
- What does a test assertion look like?
- How are test fixtures created?
- How are tests run?
- Can tests assert constraint satisfaction, geometric properties, determinacy states?
- Is there a test runner?

For a language targeting engineering design, the ability to write regression tests (this bolt pattern must have a safety factor > 2.0) is essential.

### 5.5 Backwards Compatibility and Migration

Neither document addresses:
- What happens when the language evolves (v0.1 -> v0.2)?
- Can old `.ri` files be loaded in newer runtimes?
- Is there a versioning scheme for `.ri` files?
- What happens when a standard library definition changes?

The prelude section says "Additions acceptable; removals and semantic changes are not," which is a good policy but insufficient as a migration strategy.

### 5.6 Error Messages and Diagnostics Quality

Both documents describe the information available for diagnostics (constraint violations, determinacy traces, solver failures) but neither addresses the quality of user-facing messages. For a system targeting engineers (not programmers), error messages need to be domain-specific and actionable:
- "constraint violated: wall_thickness < 2mm" is not helpful.
- "Wall thickness 1.5mm is below the minimum 2mm required for milling process X. Increase wall_thickness or change manufacturing process." is helpful.

The architecture's `ConstraintDiagnostics` structure has a `Detail` field, but what goes into that detail is unspecified.

### 5.7 Documentation Generation

The language spec defines doc comments (`///`) and `meta` blocks, but neither document describes how documentation is generated. For a library ecosystem ("minimal core, rich libraries"), documentation generation is critical.

---

## 6. Top Recommendations

Ranked by impact on the success of the overall system:

### 1. Specify the `connect`/`chain` implementation story (Critical)

Connections are the primary assembly composition mechanism. The language spec describes what `connect` creates (connector structure, constraints, frame alignment, topology edge) but the architecture says nothing about how these artifacts enter and are managed in the evaluation graph. This is the single largest gap between the two documents. Without this, the most common user workflow -- assembling components via ports -- has no implementation path.

### 2. Address the persistent naming problem before v0.1 (Critical)

The architecture correctly identifies this as open question #10, but it should be elevated to a blocking v0.1 concern. Without persistent naming, geometry selectors (`@face`, `edges(solid)`, etc.) will break when upstream parameters change, making any constraint or connection referencing geometric features unreliable. This is not merely an inconvenience -- it undermines the core promise of parametric design. At minimum, v0.1 needs a strategy (even if limited to named features in the construction history) and an explicit description of what happens when naming fails.

### 3. Define the tooling/IDE integration interface (High)

The system is designed for interactive editing but has no specified interface between the runtime and editor tooling. Define: how the demand registry is populated from UI state, what API the property editor / constraint panel / viewport use to read graph state and subscribe to updates, and how partial/invalid source text is handled. This could be a third document, but the boundary should be specified here.

### 4. Resolve `let` overloading for values vs type aliases (High)

Using `let` for both value bindings (`let mass = volume * density`) and type aliases (`let Force = Mass * Length / Time^2`) is a concision win but creates parsing ambiguity and LLM generation errors. Consider: a separate keyword for type aliases (e.g., `type Force = ...`), or at minimum, a formal rule in the grammar distinguishing the two cases beyond naming convention.

### 5. Reduce the standard library scope in the language spec (Medium-High)

Sections 11.1-11.14 of the language spec constitute roughly 40% of the document and specify detailed API surfaces for materials, ports, tolerancing, processes, and I/O. These are domain modelling decisions, not language design decisions. They should be moved to a separate "Standard Library Reference" document. The language spec should contain only the prelude, the type system foundations (dimensions, units, geometry types), and examples sufficient to demonstrate language features.

### 6. Specify `undef` propagation semantics more precisely (Medium)

The current specification ("propagates to the extent that the result relevantly depends on the undefined input; may be contained/swallowed where downstream computation doesn't depend") is too vague for implementation. Define explicitly: what happens with `if undef then A else B`? What about `match undef { ... }`? What about `undef or true`? Short-circuit evaluation semantics for logical operators with `undef` inputs? The architecture needs a truth table for `undef` in each operator context.

### 7. Add a formal testing framework (Medium)

The `@test` annotation is mentioned but unspecified. Define: test assertions (`assert_satisfied`, `assert_determined`, `assert_within_tolerance`), test fixtures (how to create a design in a known state), test execution (how tests are discovered and run), and test output (pass/fail with diagnostics). Engineering designs need regression testing; this is not optional.

### 8. Provide a concrete plan for constraint solver orchestration (Medium)

The constraint system sections in both documents are descriptive rather than prescriptive. The architecture lists 8 open questions (C-10.1 through C-10.8) about the constraint engine. For v0.1, at minimum specify: how constraint classification works (which predicate goes to which solver), what the fallback is when no solver can handle a constraint, and how cross-domain constraints are decomposed. Without this, the "orchestrator pattern" is an aspiration, not a design.

### 9. Normalise spelling and fix the `Solid` definition inconsistency (Low-Medium)

Pick American or British spelling and apply consistently. Fix the architecture's `Solid` type table entry (line 710) to remove "bounded" from the definition, matching the corrected semantics both documents agree on.

### 10. Add a "Lifecycle of a Design" narrative (Low-Medium)

Both documents are reference documents. Neither provides a narrative walkthrough of how a designer would use Reify from start to finish: create a module, define a structure, add parameters, add constraints, connect components, run analysis, export for manufacturing. The architecture's lifecycle worked examples (bracket thickness change, guard flip) are excellent but cover only the runtime behavior of individual edits. A holistic lifecycle narrative would serve as a conceptual bridge between the two documents and as an onboarding resource.

---

## Summary Assessment

This is a well-designed system with clear thinking, consistent principles, and honest acknowledgment of open problems. The core abstractions -- the evaluation graph with immutable snapshots, the determinacy spectrum, the demand-driven scheduling model, the multi-kernel geometry dispatch -- are sound and mutually reinforcing.

The primary risks are: (1) the substantial amount of open research in constraint orchestration and geometry persistent naming, (2) the missing specification of the `connect`/assembly composition implementation, and (3) the absence of a tooling integration interface for the interactive editing workflow that is the system's primary use case.

The language design is ambitious but coherent. The 43 keywords, four entity kinds, and rich standard library create a steep initial learning curve, but the payoff -- a unified framework from sketch to manufacturing spec -- justifies the investment if the implementation can deliver on the runtime promises.

Recommendation: greenlight implementation of the core evaluation graph, type system, and basic constraint checking. Hold on `connect`/assembly composition and geometry selectors until the persistent naming strategy is resolved. Separate the standard library spec from the language spec to allow them to evolve independently.
