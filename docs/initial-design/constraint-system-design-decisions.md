# Constraint System: Design Decisions

**Status:** Language-semantic design complete — ready for implementation architecture informed by solver research  
**Version:** 0.1 — First crystallization from constraint system design sessions  
**Builds on:** `ontology-design-decisions.md` v0.1, `type-system-design-decisions.md` v0.1, `syntax-design-decisions.md` v0.1

---

## 1. Design approach

The constraint system design was approached demand-side first: what does the language need from an ideal constraint engine, independent of what existing solvers provide? This establishes the semantic requirements that any implementation must satisfy. Implementation architecture — solver selection, dispatch strategies, integration patterns — is deferred to a subsequent phase informed by research into existing approaches.

The central finding is that **optimization and constraint solving are unified at the language level**. Optimization is not a separate system layered on top of constraint satisfaction — it is constraint-oriented resolution of `auto` parameters, expressible with existing language primitives (constraints, fields, `auto`, and the `@optimised` hook). No new core language features are required.

---

## 2. The constraint engine's role: orchestrator

The constraint engine is an **orchestrator**, not a monolithic solver. It dispatches to specialised sub-solvers and manages their interaction.

**Rationale:** The language asks the constraint engine to handle several qualitatively different domains simultaneously:

- **Dimensional/parametric constraints** — numeric relationships between scalar values with units (`wall_thickness > 2mm`, `grip_length == sum of plate thicknesses`). The bread and butter.
- **Geometric constraints** — spatial relationships (coincidence, parallelism, tangency, distance, angle) operating on geometric entities (points, curves, surfaces, bodies). What traditional CAD constraint solvers handle.
- **Logical/combinatorial constraints** — discrete choices, boolean gating, type selection (`head_type == Hex or head_type == Socket`, conditional sub-structure presence).
- **Cross-domain constraints** — relationships that span multiple domains simultaneously. A DFM constraint like "internal corner radius must exceed the minimum tool radius for the specified milling machine" connects geometric features, manufacturing process parameters, and tool specifications in a single predicate.

No single solver handles all of these well. The orchestrator pattern — dispatching to specialised sub-solvers for each domain and managing their interaction — is the only tractable architecture for the full constraint language.

**This orchestrator pattern is expected to be dominant across the entire runtime**, not just the constraint system. The language will integrate diverse representations, algorithms, sub-engines, and kernels (B-rep and implicit geometry, deterministic solvers and ML heuristics, etc.). Using the right one, or the right combination, and keeping them in sync, requires the same careful orchestration at every layer. The constraint engine is the first and most critical instance of this pattern.

**Alternatives considered:**
- Monolithic solver supporting all constraint domains (rejected: no existing solver covers the full space; building one would be an enormous and unnecessary undertaking when composition of existing solvers is viable)
- Separate uncoordinated solvers per domain (rejected: cross-domain constraints are first-class in the language and cannot be handled without coordination between solvers)

---

## 3. Constraint domains and their interactions

The constraint system must handle four qualitatively different domains and, critically, the **interactions between them**.

### 3.1 Dimensional/parametric constraints

Numeric relationships between typed, dimensioned scalar parameters. These form the majority of constraints in most designs:

```
constraint wall_thickness > 2mm
constraint head_diameter > shank_diameter
constraint grip_length == plate_a.thickness + plate_b.thickness
    + washer_top.thickness + washer_bottom.thickness
```

Standard numeric constraint satisfaction. Well-understood algorithms exist. The dimensional analysis type system (ontology §2.2) guarantees that operands are dimensionally compatible before the solver ever sees them.

### 3.2 Geometric constraints

Spatial relationships between geometric entities. These are what traditional parametric CAD constraint solvers handle:

```
constraint Coincident(hole.center, bolt.axis_point)
constraint Parallel(face_a.normal, face_b.normal)
constraint distance(shaft.axis, bore.axis) == 0mm
```

The language's `@optimised` hook (ontology §2.3) is designed for exactly this sub-problem. The language-level definition of `Coincident` is `distance(a, b) == 0mm`, but the optimised implementation is a geometric kernel's native coincidence constraint with specialised solvers.

### 3.3 Logical/combinatorial constraints

Discrete choices and boolean logic:

```
constraint bolt.head_type == HeadType.Hex or bolt.head_type == HeadType.Socket
constraint load > 10kN implies bolt.grade >= 10.9
constraint use_seal == true implies seal.material.max_temp >= operating_temp
```

These involve enumeration, backtracking, or SAT-style reasoning rather than continuous numeric solving.

### 3.4 Cross-domain constraints

The most demanding and most important category. These span multiple domains in a single predicate:

```
constraint def DFM_Milling {
    param part : Structure
    param machine : MillingMachine

    // Geometric + parametric + occurrence domain interaction
    forall feature in part.internal_corners:
        feature.radius >= machine.min_tool_radius
    forall wall in part.walls:
        wall.thickness >= wall.depth / machine.max_wall_aspect_ratio
    part.max_depth <= machine.z_travel
}
```

Cross-domain constraints are first-class in the language and are the primary reason the engine must be an orchestrator rather than a collection of independent solvers. The orchestrator's core responsibility is decomposing cross-domain constraints into sub-problems, dispatching them to the appropriate sub-solvers, and managing the feedback between sub-solvers when their domains are coupled.

---

## 4. The checking → solving → proposing spectrum

The constraint engine must support three qualitatively different modes of operation, corresponding to different points on the design's determinacy spectrum.

### 4.1 Checking

Given a fully determined design, verify that all constraints hold. Every parameter has a value; evaluate every predicate; report violations.

This is the simplest mode and the minimum viable capability. Even naive implementations can do this.

### 4.2 Solving

Given a partially determined design with `auto` parameters, find values for the `auto` parameters that satisfy all constraints. This is classical constraint satisfaction, potentially nonlinear and mixed continuous-discrete.

This is the mode that makes `auto` meaningful. Without it, `auto` is just a marker for future manual resolution.

### 4.3 Proposing

Given a highly underdetermined design (early-stage, extensive `undef`), provide useful feedback:

- What is constrainable given current information?
- What is in conflict?
- What would need to be determined to make progress?
- What are reasonable values for undetermined parameters?

This is the most ambitious mode and the most important for the `undef`-heavy early design workflow the language is built for. The determinacy propagation semantics already in the ontology (§3.2, §7.1) provide the foundation — the engine needs to integrate with `undef` propagation and dependency tracking to support this mode.

### 4.4 Graceful degradation across the spectrum

The three modes form a graceful degradation hierarchy. If the engine cannot optimally solve (§4.2), it can still check (§4.1). If it cannot fully propose (§4.3), it can still solve what's solvable and report what's not.

**Priority ordering for implementation:**

1. **Works** — correct and robust enough. Checking and basic solving for small problems.
2. **Good** — rich diagnostics and progress along the checking → solving → proposing spectrum for small-to-medium problems.
3. **Fast & usable** — no direct UI lag; minimal meaningful response latency; strongly concurrent evaluation; useful (but not misleading) partial results as they stream in; complete answers quickly enough to avoid context-switching (~<5s target); GPU offload where it provides significant benefit; subtle but rich UI signalling of evaluation state and expected completion.
4. **Large** — big problems; robust partitioning and algorithmic cost scaling control; optimising for space as well as time; hardened core systems; graceful degradation under excessive resource demands; user-configurable recomputation triggers; full exploitation of available hardware (all CPU threads, GPU offload, multi-machine compute, elastic backend).

---

## 5. Optimization as constraint-oriented `auto` resolution

### 5.1 The core unification

**Constraint satisfaction** finds a point in a feasible region. **Optimization** finds the best point in a feasible region. The feasible region is defined by constraints (which the language already has). The preference ordering is defined by a function from the design parameter space to a scalar (which is a field in the language's sense — a mapping from a domain to a codomain).

Therefore: **optimization is constraint solving oriented by a merit field.** It requires no new core language features.

### 5.2 `minimize` / `maximize` as syntactic sugar

`minimize` and `maximize` are syntactic sugar for creating an optimization constraint on the enclosing entity:

```
// Surface syntax
structure def LightweightBracket : RigidMechanical {
    param thickness : Length = auto
    param material : Material = auto
    constraint thickness >= 2mm
    minimize mass
}
```

The sugar expansion creates an optimization constraint — a predicate that, informally, asserts: "the resolved values of `auto` parameters in this scope must be such that no feasible alternative achieves a lower (or higher) value of the specified merit expression." This is a different *kind* of constraint from `thickness >= 2mm` (it quantifies over the feasible set rather than evaluating locally), but it is expressible as a predicate over the design parameter space and composes via the existing constraint system.

The `@optimised` hook ensures that the implementation dispatches to actual optimization algorithms rather than naively evaluating the quantified predicate.

### 5.3 The merit field's domain

The merit function in optimization maps from the **design parameter space** — the set of all `auto` parameter combinations within a scope — to a scalar. This is a high-dimensional abstract space, not ℝ³.

The language's field definition is already generic over domain and codomain types. A merit function is a field whose domain is the implicit parameter tuple of the enclosing scope. This is a first-class use case for the field abstraction, not an abuse of it — field generality over arbitrary domains was a deliberate design choice.

### 5.4 Multi-objective optimization

Real engineering involves competing objectives. The language supports multiple approaches:

**Weighted sum** — the simplest case. Multiple objectives are combined into a single scalar merit field:

```
minimize 0.6 * mass + 0.4 * cost
```

This collapses the multi-objective problem to a single-objective one. It cannot find non-convex regions of the Pareto frontier, and the weights encode preference in ways that are hard to calibrate, but it is by far the most common practical approach.

**Lexicographic ordering** — priority-based: "minimize mass; among equal-mass solutions, minimize cost." This is a strict priority ordering, not a weighting. Syntax and semantics to be determined, but it maps naturally to an ordered list of objectives.

**Pareto exploration** — "show me the trade-off surface." The output is a set of designs, not a single optimum. This is fundamentally an analysis/exploration mode rather than a single `auto` resolution. It belongs in tooling rather than core language semantics.

**Design decision:** Weighted-sum is the default mechanism for `minimize`/`maximize`. Lexicographic ordering is an explicit extension. Pareto exploration is a tooling concern, not a language-level construct.

### 5.5 Discrete, continuous, and mixed problems

`auto` resolution may search continuous spaces (wall thickness), discrete spaces (bolt selection, head type), or both simultaneously (choose a bolt AND optimise wall thickness). These require qualitatively different algorithms — gradient-based methods for continuous, combinatorial search for discrete, and mixed-integer nonlinear programming for combined problems.

This does not break the language-level unification. Optimization is still "best feasible point in the constraint-defined region" regardless of parameter types. The solver orchestrator dispatches to appropriate sub-solvers based on the types of the `auto` parameters involved. The language semantics are uniform; the implementation strategy varies.

**Alternatives considered:**
- Separate optimization system layered on top of constraint solving (rejected: the interactions between constraint satisfaction and optimization are too rich for a clean separation; they must be integrated)
- Optimization as a separate language construct distinct from constraints (rejected: expressible with existing primitives — constraints, fields, `auto`, and the `@optimised` hook — with `minimize`/`maximize` as sugar)

---

## 6. Scope, nesting, and bottom-up resolution

### 6.1 Scope-level objectives

Optimization objectives are scoped to the entity that contains them. Narrowest scope wins. This matches engineering practice — "optimise the whole design for cost, given satisfaction of the other constraints" — and is strictly more powerful than parameter-level objectives.

```
structure def System {
    minimize total_cost                      // System-level objective

    sub bracket : Bracket {
        minimize mass                        // Subsystem-level objective (overrides for this scope)
    }

    sub housing : Housing {
        // No local objective — inherits system-level minimize total_cost
    }
}
```

### 6.2 Bottom-up resolution

The default resolution strategy is bottom-up. The most proximate (narrowest-scope) objective applies first. Once a scope's `auto` parameters are resolved, the results are fixed from the perspective of enclosing scopes:

1. Resolve `auto` parameters in leaf scopes, using their local optimization objectives.
2. Treat resolved leaf scopes as fixed — their parameters are now determined values.
3. Resolve `auto` parameters in parent scopes, using the parent's objectives, with child results as given.
4. Continue upward to the root.

This is both intuitive and computationally efficient — each scope's problem is smaller than the global problem, and resolution at each level produces a fixed result that simplifies the enclosing problem.

### 6.3 When bottom-up resolution is an approximation

Bottom-up resolution is exact when scopes are **uncoupled** — when a child scope's optimization does not affect the parent scope's feasible region in ways that interact with the parent's objective.

It is an approximation when there is **coupling** — when a child scope's locally optimal result is not globally optimal because the parent scope's objective creates preferences that the child scope's local objective didn't account for. Example: a bracket locally optimised for minimum mass selects expensive lightweight material; the system-level objective is minimum cost; a heavier, cheaper bracket would have been globally better.

**Design decision:** Bottom-up resolution is the default because it is tractable and usually adequate. The implementation should detect coupling (when a parent-scope optimiser encounters an active constraint boundary contributed by a child scope's resolved values) and surface it as a diagnostic. The designer can then choose to broaden the optimisation scope, accepting additional computational cost for better global results. This is analogous to how engineers actually work: design subsystems, integrate, discover interactions, iterate. The language makes the iteration points visible.

### 6.4 Conflicting objectives

If two objectives in the same scope conflict (e.g., `minimize mass` and `maximize stiffness` without a weighting), this is an error — analogous to conflicting constraints. The designer must combine them into a single weighted objective or establish a lexicographic priority.

Objectives that nest without conflict are fine: a child scope minimises mass while the parent scope minimises cost. The child's result feeds into the parent's problem as a determined value.

---

## 7. Default objectives and the purpose connection

### 7.1 The problem

Most `auto` parameters won't have an explicit merit function. The designer writes `length = auto` and means "whatever works." Without an objective, `auto` resolution is underdetermined — there may be many feasible values, and the system has no basis for choosing.

### 7.2 Default objectives via purpose

The ontology's **purpose** concept (§8.2) provides the natural home for default optimization objectives. A purpose is already a named determinacy predicate — a set of requirements specifying which parameters must be determined for a particular downstream use. Extending purpose to include a default optimization objective is a natural generalisation:

```
purpose def manufacturing_ready {
    // Determinacy requirements
    require all geometric_params determined
    require all material_params determined

    // Default optimization objective
    minimize cost
    subject_to all safety_constraints with margin >= 1.5
}
```

When a design is evaluated against a purpose, the system knows both what needs to be determined and what to optimise. This ties the optimization objective to the *reason* for evaluating — the right place for it conceptually, since you optimise for different things depending on what you're trying to achieve.

### 7.3 Implicit default objective

If no explicit purpose or objective is specified, a default purpose applies. The standard library provides this default. The expected default is a robustness-oriented objective: among feasible values, prefer those that maximise distance from constraint boundaries (centrality in the feasible region). This produces solutions that are robust to perturbation without requiring the designer to specify a domain-specific objective.

Domain libraries can register smarter defaults for specific parameter types via the `@optimised` hook (bolt length selection snaps to the next standard size, sheet thickness snaps to available stock, etc.).

### 7.4 Legibility and override

Default objectives must be:

- **Legible** — the designer can always query what objective is governing a given `auto` resolution, and where it came from (explicit local, inherited from parent scope, purpose-level default, or global default).
- **Overridable** — any scope can override the inherited or default objective with a local `minimize`/`maximize` directive. The override is clean and total within that scope.

---

## 8. Strict and free `auto`

### 8.1 Strict `auto` (default)

In strict mode, `auto` resolution requires that the resolved value is **well-determined** — either uniquely determined by constraints, or uniquely optimal under the applicable objective. If neither condition holds (multiple feasible values, no distinguishing objective), strict `auto` is an error.

**Rationale:** `auto` is a decision — "I want this resolved." Strict mode holds the system to actually resolving it. If the system can't determine a unique best resolution, that's information the designer needs: the problem is either underconstrained (needs more constraints or an objective) or the objective surface is flat (multiple equally good solutions). Both are worth knowing.

**Ties and flat regions:** When multiple values are equally optimal, a deterministic tiebreaking rule applies (e.g., lexicographic ordering, closest to conventional default). The specific rule matters less than it being deterministic and documented.

### 8.2 Free `auto`

Free mode is an explicit opt-in for exploration. It returns *a* feasible solution (satisfying all constraints, optimal if an objective exists, arbitrary if not) and triggers a warning that the result is not uniquely determined.

```
param wall_thickness : Length = auto(free)
```

Free mode is useful for early-stage design exploration — "just give me a plausible design so I can see what the space looks like." It should not be the default because it silently hides underdetermination.

### 8.3 Interaction with default objectives

With a global default objective in place (§7.3), strict `auto` is well-defined almost everywhere — the robustness-oriented default provides a preference ordering even when no domain-specific objective is specified. Strict `auto` fails only when the feasible set is genuinely degenerate (discrete ties, perfectly symmetric configurations). In these cases, the diagnostic is meaningful: "this design has a symmetry that no objective breaks."

**Design decision:** Strict is the default. Free is an explicit modifier. The expectation that a default objective is always in scope means strict mode is practical, not merely aspirational.

---

## 9. Interaction between optimization and `undef`

If some parameters are `undef` (not `auto`), they cannot be optimised but they affect the feasible region. The optimiser's result for the `auto` parameters is **conditional on** the `undef` parameters.

Conceptually, the solution is a field from the `undef` parameter space to the optimal `auto` values. In practice, this means resolved `auto` values may need to be **re-resolved** when `undef` parameters later become determined.

This falls out of the existing dependency tracking and `undef` propagation semantics (ontology §3.2, §7.1). A child scope's resolution is fixed *given its `undef` inputs*. When those inputs change (because an upstream `undef` parameter becomes determined), dependent `auto` resolutions are invalidated and re-resolved. The incrementality architecture (§10.2) must support this efficiently.

---

## 10. Open questions for implementation architecture phase

### 10.1 Solver dispatch strategy

How does the orchestrator decompose a mixed-domain constraint set into sub-problems, select sub-solvers, and manage feedback between them? What existing solvers are candidates for each domain (geometric constraint solvers, numeric NLP solvers, SAT/SMT solvers, mixed-integer solvers)? How are cross-domain constraints handled — ADMM-style decomposition, iterative solving with fixed-point convergence, or something else?

### 10.2 Incrementality

How does the engine respond to incremental changes (parameter modification, constraint addition/removal, sub-structure substitution) without full re-solving? What dependency tracking infrastructure is needed? How does this integrate with `undef` propagation?

### 10.3 Scalability and partitioning

How does the engine exploit the hierarchical structure of the design (containment tree, module boundaries) for problem decomposition? What are the algorithmic cost scaling characteristics? How does the engine handle designs with thousands of structures and tens of thousands of parameters?

### 10.4 Non-convexity and multiple solutions

How does the engine handle non-convex feasible regions and multiple local optima? What strategies (multi-start, global optimisation algorithms, branch-and-bound for discrete variables) are employed? How does the engine communicate confidence in the optimality of its solutions?

### 10.5 Quantifiers and collection constraints

What is the strategy for `forall` and `exists` over collections? Eager expansion (generate N individual constraints), lazy evaluation (check on demand), or hybrid? How does this interact with dynamically-sized collections (bolt patterns parameterised by count)?

### 10.6 Geometric constraint sub-problem

How deeply does the orchestrator understand geometry versus treating geometric constraints as opaque calls to a geometry kernel? What is the interface between the orchestrator and geometry-native constraint solvers? How is the `@optimised` hook implemented for geometric constraint specialisation?

### 10.7 The field-to-geometry bridge

Geometry can be represented as an SDF field (ontology §2.4), but most engineering geometry also needs boundary representation for manufacturing, GD&T, and feature recognition. How does a single structure hold both representations? How do they stay consistent? How does the constraint system span both?

### 10.8 ML and heuristic integration

Where do learned heuristics (ML-based initial guesses, surrogate models for expensive simulations, AI-assisted `auto` resolution) fit in the orchestrator architecture? How are they validated against the formal constraint system? What is the trust model?

---

*Document generated from constraint system design sessions. Intended as a living specification to be refined through subsequent design phases.*
