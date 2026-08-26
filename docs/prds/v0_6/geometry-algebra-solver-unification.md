# PRD — Geometry⇄algebra solver unification (one continuous residual core)

**Milestone:** v0_6 · **Status:** active · **Approach:** B + H (contract + two-way boundary tests) · **Authored:** 2026-08-26
**Provenance:** P2 of the four-PRD solver programme spawned from Leo's 2026-08-26 solver-integration session. Operator rulings O1–O4 are **settled** and are not re-litigated here; this document is their design detail.
**Code anchors** verified against main `2128c3692c` (2026-08-26). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

> *"a malformed system is refused whole rather than lowered with the offending declaration quietly dropped. That distinction is the entire point: a partial lowering still solves, and what comes back is a plausible answer to a question the caller did not ask."*
> — `crates/reify-constraints/src/sketch.rs`, the rationale above `SketchSystem::validate`. This PRD generalises that discipline from the 2D sketch table to every constraint the solver touches.

---

## 1. Goal & user-observable surface

A designer writes design intent — `constraint std::distance(a, b) >= clearance`, `relate { tangent(pin, boss) }`, `minimize rope_length` — and the solver either **solves it**, or **names precisely what it cannot lower and why**, while everything else in the model still solves. Today the third outcome dominates: the model silently reports success having constrained nothing.

Four sentences a user can say after this PRD that they cannot say today:

1. *"`constraint std::distance(a, b) >= clearance` drove my part into position."* Today it kills the entire resolve (or, if the call is behind a `let`, silently routes to a different solver and gives a different answer).
2. *"`relate { tangent(cyl_a, cyl_b) }` moved the pin until the cylinders touched."* Today `tangent` type-checks, publishes `removes 2`, contributes **zero** residual rows, and `reify check` prints `All constraints satisfied.` at exit 0 with the pose left at identity.
3. *"The solver told me it could not lower `coincident(axis, plane)` — and solved the rest of my model anyway."* Today one unlowerable constraint discards every already-solved component and emits an **uncoded warning** with no span and no constraint id.
4. *"It said it ran out of budget, so I raised the budget."* Today iteration-limit exhaustion, damping-ceiling breakout, and genuine geometric inconsistency all print the same sentence.

**Scope in one line (O3-scoped):** unify assembly-pose solving and dimensional solving onto **one** in-tree damped/trust-region Gauss–Newton core over one variable vector (poses ⊕ scalars) and one residual vector (relations ⊕ constraints ⊕ slacks). The 3D SolveSpace *expression* path retires. **libslvs is kept** for the 2D sketch substrate. CP-SAT stays the discrete backend. This is not a from-scratch solver and does not compete with SolveSpace in the sketch domain.

---

## 2. Background — why this exists, and why now

### 2.1 This is a named future, not greenfield

Two in-tree records already charter it:

- `docs/prds/v0_6/constrained-2d-sketch.md` §11 *Out of scope (named future work)*: **"Solver consolidation (three geometric solvers: relate-solve GN, sketch/libslvs, legacy pattern path) — future; breadcrumbs only here."** The same sentence is repeated verbatim in the doc comment above `recognize_pattern` in `crates/reify-constraints/src/solvespace.rs`.
- `docs/design/geometric-relations.md` §12 defers **"Geometry-in-the-loop solving … a future PRD that would re-introduce a bounded fixpoint scoped to the Resolution node."**

Leo's O3-scoped ruling **refines** the first: consolidation covers relate-solve's Gauss–Newton, the legacy pattern path, and `DimensionalSolver`'s Nelder-Mead — **not** the sketch path. §11 of that PRD should be read with that refinement (see §8, correction leaf).

### 2.2 The taxonomy is already ruled — this PRD does not invent one

`docs/design/geometric-relations.md` §2 (decision log §14, locked 2026-06-08, interactive) fixes the **datum lattice**: `Direction` (codim 2) · `Point` (3) · `Axis` (4) · `Plane` (3) · `Frame` (6) · `Scalar`. §2.2 fixes the feature→datum trait bundle: `Planar → Plane`; `Cylindrical → Axis + radius`; `Conical → Axis + apex + half_angle`; `Spherical → Point(centre) + radius`; `Linear → Axis`; `ArcBounded → Axis + Point + radius`; `Revolute → Axis`; `Extruded → Direction`; `Vertex → Point`. §3.1 fixes the law `coincident(X, X)` removes `codim(X)`.

**The O1 "AnalyticForm taxonomy" is therefore the datum lattice, and surface kinds enter as *attributes on a datum*, not as new datum kinds.** That distinction is load-bearing and is the reason `tangent` has no residual today (§2.4). This PRD adds no competing taxonomy; §7.1 states the table's index precisely.

### 2.3 The trap, measured

`ConstraintClassifier::classify` (`crates/reify-constraints/src/classifier.rs`) routes by **function name string** — `is_geometry_qualified_name` matches 11 literal `std::*` names. Anything it flags reaches `SolveSpaceSolver`, whose `recognize_pattern` matches exactly **five** expression shapes (`PtPtDistance`, `Angle`, `Parallel`, `Perpendicular`, `Coincident`). Anything unmatched returns `SolveResult::NoProgress`, and `SolverRegistry::solve_inner` **returns immediately from inside its component loop**, discarding `merged_values` accumulated from every component already solved.

Four corrections to the received framing, each measured at this HEAD, each changing what the fix must be:

- **`std::geo::*` / `std::geometry::*` are dead prefixes.** The compiler builds every stdlib qualified name as `format!("std::{}", name)`; both `starts_with` arms are unreachable in production and appear only in tests. Do not design against them.
- **`→ Geometric` is conditional.** A `Type::Bool` `ValueRef` or Bool literal anywhere in the expression demotes the whole thing to `CrossDomain`; a mixed-domain *component* is `CrossDomain`; and `CrossDomain` routes to the `fallback` slot, which is `None` in `SolverRegistry::production()` — i.e. back to `DimensionalSolver`. Three independent widenings (`DomainFlags::into_domain`, component unanimity in `decompose_into_components_with_reads`, `widen_domain`/`domain_of_auto`) all feed this.
- **The `let` boundary changes the answer.** `constraint distance(a,b) >= g` (call inline) classifies `Geometric` → SolveSpace → `NoProgress` → whole-resolve kill. `let gap = distance(a,b); constraint gap >= g` puts only `ValueRef`s in the constraint tree → classifies `Dimensional` → **solves**. Probe-confirmed. Two semantically identical models get different solvers and different answers. This single fact is the sharpest argument that routing must be capability-based, not name-based.
- **The vocabulary is out of sync three ways.** `std::fasten` is in `RELATION_FN_NAMES` but missing from the classifier list (→ `Dimensional`). `std::angle` (the arity-3 DRIVE form) is likewise absent, though the classifier's own doc comment claims it is present. And `.contains("parallel")` is tested before the `antiparallel` case, so **`antiparallel(a, b)` is lowered as `SLVS_C_PARALLEL`** — the opposite orientation constraint. Six of the eleven flagged names (`tangent`, `concentric`, `flush`, `offset`, `on`, `antiparallel`) have no recognizer arm at all and are therefore *guaranteed* whole-resolve kills.

### 2.4 The silence, measured

The failure that matters most is not the loud kill — it is the quiet success.

`residual_dispatch` (`crates/reify-constraints/src/relate_solve.rs`) ends in `_ => Vec::new()` under the comment *"tangent (surface-conditional) and uncurated names contribute no rows."* A zero-row relation is not refused; it is **absorbed**: `relation_jacobian` returns no rows, `partition_driving_set` scores `rank_contribution == 0` and files it under `redundant`, and the post-solve verifier `max_relation_residual` iterates zero components and returns `0.0 ≤ assertion`. The relation passes, vacuously.

Probe, committed as PRD evidence at `tests/prd-gate/fixtures/solver_unification_tangent_silent_accept.ri`: two `tangent` relations driving an `at auto` sub, no other relations.

```
$ reify check p_c_no_residual.ri
All constraints satisfied.
[exit 0]                       (stderr empty)
$ reify eval p_c_no_residual.ri
PcNoResidual.pin.__auto_pose = frame(point(0 m, 0 m, 0 m), [1, 0, 0, 0]q)
$ reify build p_c_no_residual.ri -o out.step
Wrote out.step
```

All six pose DOF unconstrained, an identity pose emitted, and three surfaces agreeing that everything is fine. Two controls separate "converged on nothing" from "never ran": a bare `at auto` with no `relate` block emits **no** `__auto_pose` cell at all, and a single `concentric` (ΔDOF 4, leaving 2 free) solves and reports no `W_UNDERDETERMINED`.

**The same silent-zero path is reachable seven ways**, every one of which the table in §7.1 must close: the `_ =>` catch-all in `residual_dispatch`; every *mixed-kind* pair in `coincident_residual` (diagonal-only `match`); an unrecognised host in `on_residual`; operand kind-extraction failure (`axis_parts`/`plane_parts`/`dir_of`/`origin_of` returning `None`); a degenerate zero-length direction (`unit3` → `None`); a missing scalar operand on `distance`/`angle`/`offset`; and `frame_coincidence_residual`'s log-map guard, which drops all six rows rather than three. Adjacent to these, `transform_datum`'s `_ => v.clone()` silently treats a *moving* datum of an unhandled kind as fixed, and `pick_ab` makes a relation whose two operands belong to the same moving sub pose-invariant — hence rank 0, hence "redundant".

### 2.5 Why `tangent` in particular has no rows — the radius is dropped three times

`tangent(cylinder, plane)` is `distance(cyl.axis, plane) − cyl.radius = 0`. The radius never arrives:

1. `analytic_surface_datum_to_value` (`crates/reify-kernel-occt/src/lib.rs`) receives an `AnalyticSurfaceDatum { origin, direction, scalar1, scalar2, kind }` where the OCCT wrapper has already put `cyl.Radius()` in `scalar1` (and `sph.Radius()`, `cone.SemiAngle()`, apex distance likewise) — and reads **neither scalar**. Kind 1 (Cylinder) and kind 2 (Cone) both become a bare `Value::Axis`; kind 3 (Sphere) becomes a bare `Value::Point`.
2. `datum_from_value` (`crates/reify-eval/src/feature_datum.rs`) hard-codes `radius: None`. `Datum::Axis.radius` is `#[allow(dead_code)]` and is never `Some` anywhere in tracked source; `Datum::Point` and `Datum::Plane` have no radius slot at all.
3. `Operand { sub, datum: Value }` (`relate_solve.rs`) admits no attribute channel, and `is_datum` accepts only `Axis | Plane | Direction | Point | Vector | Frame`.

By the time `residual_dispatch` runs, **a cylinder is indistinguishable from a line and a sphere from a point.** That is task #5588's subject, and it is a precondition of the table, not a follow-on to it (§8, leaf α).

Two adjacent defects surfaced while measuring this and are folded into α: the Sphere arm ships `Point3{0,0,0}` in the `direction` field — a non-unit zero vector in a slot the rest of the pipeline treats as a unit direction, harmless today only because the Rust side discards it; and **Torus is absent** from the analytic classifier although `reify_ir::FaceSurfaceKind` already names it (§7.1 aligns the two vocabularies).

### 2.6 Inequalities do not compose with equalities — a provable, exact defect

Measured this session, with no geometry involved at all:

| model | result |
|---|---|
| `w == 10mm` | `w = 0.01 m` ✓ |
| `w >= 8mm` | `w = 10 m` — **ten metres**, strict `auto`, exit 0, no warning |
| `w >= 8mm; w <= 12mm` | `w = 0.01 m` ✓ (exact midpoint) |
| `w >= 8mm; w == 10mm` | `error: constraints could not be satisfied (max absolute residual: 5.00e-7)` |
| `w >= 1mm; w == 10mm` | identical, byte for byte — **the residual does not depend on the slack** |
| `w <= 20mm; w == 10mm` | identical again |
| `w >= 8mm; w <= 12mm; w == 10mm` | `w = 0.01 m` ✓ |

The root cause is exact and derivable. With ≥1 inequality slack and no author objective, `build_centrality_objective` synthesises `Maximize(min_j slack_j)`. `ConstraintCostFunction::cost` then minimises

```
cost(w) = −slack(w)  +  PENALTY_WEIGHT · violation(w)
        = −(w − 8mm) +  1e6 · (w − 10mm)²
```

whose stationary point is `w* − 10mm = 1/(2·PENALTY_WEIGHT) = 5×10⁻⁷`. `comparison_residual` then measures `|w* − 10mm| = 5.00e-7`, which exceeds the dimension-blind `FEASIBILITY_THRESHOLD = 1e-12`, and the solve is declared **infeasible on a feasible model**. With a *single* slack the objective gradient is ±1 everywhere, so the offset is independent of the bound — exactly as measured. With two opposing slacks the min-fold's gradients cancel at the interior point and the equality is met — exactly as measured. The discriminating three-case probe above was run to confirm this before it was written down, and both halves are committed: `tests/prd-gate/fixtures/solver_unification_ineq_eq_penalty_offset.ri` and its control `tests/prd-gate/fixtures/solver_unification_ineq_eq_two_sided_control.ri`.

**The lesson generalises past this one bug:** a finite penalty weight makes every constraint *soft*, so any objective — including one the system synthesised on the user's behalf — can buy its way out of a constraint for a bounded price. No tolerance tuning fixes that; only exact constraint handling does (§7.4).

Two further facts from the same measurement, both of which the new core must not reproduce: the `w >= 8mm → 10 m` answer is not a midpoint but the **ceiling of the dimension-default box** (`default_bounds_for` gives `LENGTH → (1e-6, 10.0)`) reached by maximising the sole slack; and `build_centrality_objective` folds `min` as nested `Conditional`s with **O(2ⁿ) expression-tree growth** in slack count (its own source warns above 10 slacks), so the default objective is simultaneously the least smooth and most expensive encoding available.

### 2.7 The demand evidence: the table has been hand-written five times

Five partial versions of the same (form × relation) → behaviour table exist in tree. They disagree, and three of them fail silently:

| # | Table | Location | Keyed on | Miss behaviour |
|---|---|---|---|---|
| 1 | analytic-form extraction | `face_analytic_datum` / `edge_analytic_datum` (occt_wrapper) + `analytic_surface_datum_to_value` | `GeomAbs` kind | **throws** (typed) — but then drops the scalars |
| 2 | 2D sketch entity × constraint | `SketchConstraint::operands` (`sketch.rs`) | `SketchSlotKind × SketchEntityKind` | **`WrongEntityKind{expected, found, span}`, whole-system refusal before emit** ← the exemplar |
| 3 | 3D relation residual dispatch | `residual_dispatch` + `coincident_residual`/`distance_residual`/`on_residual` (`relate_solve.rs`) | relation name, then `(Value, Value)` kind pair | **`Vec::new()` — silent, absorbed as "redundant"** ← the defect |
| 4 | relation ΔDOF + operand policing | `relation_delta_dof`, `relation_delta_dof_kinds`, `relation_operand_datum` (`relation_signatures.rs`) | name × `arg_ty(i)` | `None` (gradualism), no diagnostic |
| 5 | datum projection | `datum_projection_result_type` (`datum_projection.rs`) | receiver `Type` × member | **`Resolved \| Unavailable \| Ambiguous{suggestions}` + redirect hint** ← the best return shape |

Tables 3 and 4 are two hand-maintained statements of the same (relation × kind) grid, and **the mechanism that could detect their divergence is dead in production**: `RelationInstance.nominal_delta_dof` exists to cross-check the published codimension against the Jacobian-measured rank, but `build_relation_instances` hard-codes it to `None` because `relation_delta_dof` is `pub(crate)`. So "published `removes 2`, measured rank 0" — precisely `tangent` — is undetectable. `individual_rank` is computed on every `RelationRank` and read by nothing.

A sixth, purely nominal taxonomy already exists in the IR and is consumed by the selector vocabulary: `reify_ir::FaceSurfaceKind` (`Plane | Cylinder | Cone | Sphere | Torus | BezierSurface | BSplineSurface | OffsetSurface | Other`) and `EdgeCurveKind`. It overlaps table 1 by four of nine surface forms and is connected to it by no shared type.

The sixth hand-written version is outside the language entirely. `prj/printer_v01/printer.ri` (1762 lines on main, 145 `constraint` statements, **zero** `relate`, **zero** `at auto`) carries a prose catalogue headed *"THE FIVE TANGENCY RULES"* which in fact enumerates **seven**, rule 7 reading in part: *"interior angle φ with c = cos φ gives rope tangents t = r(1+c)/sqrt(1−c²) from the corner along each line and disc centre at k = r/sqrt(1−c²) per bisector component (all sqrt-algebra, no trig)."* Those rules are then executed by hand as `let` arithmetic, and checked by an out-of-language reimplementation — `prj/printer_v01/tools/v2_check.py`, whose own docstring says it *"Mirrors the DERIVATIONS in printer.ri's `DriveTendons` (not its literals), so a disagreement means the .ri and this script disagree about the tangency rules."* Its sibling `incidence.py` exists solely because there is no `tangent(disc, curve)` predicate. `tools/README.md` records why they exist: *"the 2026-07 hand-verified layout shipped nine interfering pairs, and the 2026-08 rewrite of it introduced a wrap-sense error that a human caught by eye after the scripts had passed it."*

The extreme case is one constraint at `printer.ri` line ~1410: a full 3D point-to-parametric-line distance, inlined, roughly 500 characters, which under a named vocabulary is `clearance(diagonal_segment, pulley_disc) >= r_pitch + rope_r + clear_min`.

---

## 3. Sketch of approach — five pillars

**P1 · One lowering table (O1).** A single total map `(relation, datum-kind pair, attributes) → row`, where every row publishes its codimension **and** its residual generator **and** its lowering arm. Arms are `Analytic`, `KernelQuery{budget}`, `Unavailable{reason}`, `Ambiguous{suggestions}` — there is no fall-through. One row, one codimension, one generator: the ΔDOF/residual divergence of §2.7 becomes unrepresentable rather than merely detected.

**P2 · One continuous core (O3-scoped).** One variable vector over *all* `at auto` pose DOF ⊕ all scalar autos; one residual vector of typed, dimension-carrying rows; trust-region Gauss–Newton over a Householder QR of the **scaled** Jacobian; derivatives from forward-mode dual numbers over `CompiledExpr` plus analytic blocks for table rows.

**P3 · Exact constraints.** Inequalities become equalities via squared slack variables, so the least-squares structure is preserved and no finite penalty weight can trade a constraint away (§2.6). Objectives leave the residual vector entirely and are handled by a reduced-gradient step in the null space the QR already produced.

**P4 · Capability routing.** `ConstraintSolver` grows a capability surface; the classifier stops matching names; the 3D SolveSpace expression path retires. `solve_sketch`, `SystemBuilder` and `slvs_sys` are untouched.

**P5 · Refusal is a first-class outcome.** Every non-`Solved` result carries a typed, coded, span-bearing cause; refusals are per-constraint and never discard a sibling component's solved values.

---

## 4. Pre-conditions for activating

| Prerequisite | State | Relationship |
|---|---|---|
| **#6653** toleranced Scalar verdicts, single-sourced with solver acceptance | pending, **high** | **Hard dependency.** The core's scaled convergence test and the engine verdict fold must be the same policy; without it a correctly-solved model still reports VIOLATED at ~1e-16 (measured on `multi_auto2`/`multi_auto6`/`auto_probe`). Leaf ζ single-sources against it. |
| **#5540** 3D tangent residuals + operand-conditional ΔDOF | pending, **high**, dispatchable | **Hard dependency of leaf γ.** #5540's amended per-combo table (sphere/plane 1, sphere/sphere 1, cyl/cyl 1, cyl/plane 2) is the authoritative row data; γ *adopts* those rows into the table rather than re-deriving them. |
| **#6659** typed per-constraint refusal (interim de-trap) | **in-progress, live claimant** | Ships the refusal *shape* with no capability change; its own text says this PRD "subsumes this trap properly via residual lowering". Leaf δ **extends** it to capability-derived refusals and adopts its diagnostic vocabulary. Not duplicated. |
| **#6523** move the solver capability probe behind the `ConstraintSolver` trait | **cancelled at decompose, absorbed into ν #6681** | `domain_of_auto` reaches into `crate::cpsat::can_enumerate`; `decompose.rs`'s own doc block calls this unsound the moment a non-CpSat solver occupies the `logical` slot and names the fix. Cancelled rather than left pending, because a pending task is *dispatchable* and would have collided with ν on `decompose.rs`. Its text — including the `domain_spec` delegation target and the `PRECONDITION on the Logical slot` doc block whose deletion marks completion — was ported into ν's `details` first; an exhaustive reverse-dependency scan (6696 tasks) confirmed nothing depended on it. |
| **#6251** every interference/clearance query derives from one non-negative kernel distance | pending, medium | **Hard dependency of leaf ο** — see the G6 hazard in §6.4. |
| **#6583** cross-sub geometry reads drop the sub's `at` placement | pending, **high**, no deps | **Hard dependency of leaf ο**, established by the decompose-time D3 run — see §7.6. A kernel query over *unposed* operands answers a question about a geometry that is not the one being assembled. |
| **#6655** HC4 interval seed boxes | pending, medium | **Soft.** Leaf κ removes the *centrality-vs-penalty* defect; the `w >= 8mm → 10 m` **box ceiling** is #6655's subject. Coordinate WITHOUT an edge, do not absorb *(corrected 2026-08-26: §9 and κ #6678's record are authoritative — the executed design wires no κ→#6655 edge; #6655's box work is consumed opportunistically)*. |
| **#5588** surface-carried radius | **rescoped at decompose to item 2 only**; pending, low, deps now [#5540, α #6668] | Item 1 (the three-layer radius drop) is **subsumed by leaf α**, whose declared extent was widened to cover all three files item 1 named. Item 2 (`<HasAxis & HasRadius>`) stays out of this PRD — but its *stated blocker was stale twice over* and the rescope records the correction: call-site trait conformance is live and wired (`check_trait_arg_conformance` → `conformance/mod.rs`'s `Type::TraitObject` arm; #2227 and #4081 both `done`), so `ty.rs`'s "deferred" comment is itself a docs-truth defect; and an intersection type may not be needed at all, since `trait Watertight : Closed + Manifold {}` already ships in `stdlib/geometry_traits.ri`. The real question is nominal-vs-structural, not blocked-vs-unblocked. |
| **#6583 / #6586 / #6592** cross-sub placement dropped; `self.sub.member` → ctor arg yields undef | pending | Not dependencies of this PRD's leaves, but they gate `printer_v01` **as a consumer**. Named in §5 rather than silently assumed. |

No novel grammar. Every surface this PRD touches — `relate`, `constraint`, `at auto`, `minimize`, the ten relation names, the arity-3 `distance`/`angle` DRIVE forms — parses and type-checks on main today. **G3 grammar gate: N/A by inspection**; the substrate questions here are all semantic and are answered in §2 and §6 from measured behaviour.

---

## 5. Consumers (G1)

| Consumer | What it consumes | Status |
|---|---|---|
| **`prj/printer_v01/` rear-drive routing** | The named tangency/clearance vocabulary that replaces rules 1–7's hand algebra and `tools/v2_check.py`'s out-of-language twin. | On main. **Blocked as a consumer by #6583/#6586/#6592**, not by this PRD — named honestly, not assumed. |
| **`docs/prds/v0_6/placement-relations-belt.md` §7.2 clearance family** (#5441–#5443) | Makes `min_clearance` / `self_clearance` **drivable** rather than verdict-only. That PRD owns the query semantics; this one owns their residual lowering. | pending; seam owned per §6 |
| **`docs/prds/v0_6/material-waste-cost-minimisation.md` (M-WASTE)** | Its stated blocker is *"the solver's objective is evaluated over the param `ValueMap` **only** … so a geometry-dependent cost objective requires an outer candidate-sweep loop"*. Leaf ο's dispatch trait is that loop's inner half. | deferred forward-stub |
| **`docs/prds/v0_6/geometric-relations.md` θ (#4388)** DOF ledger | Leaf η's QR yields the rank **and the null-space basis** in one factorisation — the twists θ names geometrically. Today rank comes from a *separate* Gram–Schmidt pass. | pending |
| **P3 multimodality PRD** | The unified core and the autodiff direction. (The interval substrate is **#6655's**, not built here — this PRD holds it as a soft edge; corrected 2026-08-26 per P3's §7.1 companion obligation.) P3's brief states it **pauses** until this document is committed. | authoring in parallel |
| **P4 legibility PRD** | The typed cause vocabulary and the Lagrange multipliers/slack values leaf μ produces. P4 renders; P2 produces. | authoring in parallel |
| **Any `.ri` author** writing `constraint std::distance(a, b) >= clearance` | Today: whole-resolve death, or a different answer depending on whether the call sits behind a `let`. | — |

**Engine-integration norm (G1 sub-check).** No new seam. The unified core occupies `engine-integration-norm.md` §3.5 (ConstraintSolver); the kernel-query arm enters through §3.1 (op-execute) for kernel probes, mirroring `ComputeDispatch`'s existing route.

---

## 6. Cross-PRD relationship (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| **P1 driver-parity** (authoring in parallel) | orthogonal | P1 owns *where* solving runs (`cmd_build`/`cmd_check` flip, determinism, cost tiers); this PRD owns *what the solver is*. The only shared artefact is #6653. | each owns its side | parallel |
| **P3 multimodality** (authoring in parallel) | produces→them | The unified core, `eval_objective_set` as the **single** objective fold (P3's stated constraint — leaf μ preserves it), residual/Jacobian access for branch-and-bound. Basin enumeration, multistart and found-basins honesty are **theirs**. | P3 owns enumeration; this PRD owns the core | P3 waits on this commit |
| **P4 legibility** (authoring in parallel) | produces→them | The typed cause enum, per-row blame attribution, slack values and Lagrange multipliers. P4 renders them in GUI/MCP/`explain`; it does not own their semantics. | P4 owns surfaces | parallel |
| `v0_6/placement-relations-belt.md` §7.2 (#5441–#5443) | both | Belt owns the clearance **query** vocabulary and its Kleene verdict semantics; this PRD owns **lowering those queries into the solve** (leaf ο). Belt's queries stay valid unchanged when not driving. | belt owns signatures; this PRD owns residuals | belt entirely unstarted |
| `v0_6/geometric-relations.md` + `docs/design/geometric-relations.md` | consumes | The datum lattice (§2), the relation vocabulary (§3), the `coincident(X,X) = codim(X)` law, the tolerance-coherence law `kernel_local ≤ solver_convergence ≤ assertion/dedup` (§2.3). Locked decisions A–O are **not** reopened. θ (#4388) consumes leaf η's null space. | design doc owns the ontology; this PRD owns the lowering | θ pending |
| `v0_6/constrained-2d-sketch.md` §11 | corrects | §11's "Solver consolidation" names three solvers; O3-scoped keeps the sketch path. **This PRD owns a one-line correction to that bullet** (§8, leaf τ's companion edit) rather than leaving a contradiction. | this PRD | — |
| `v0_6/constraint-solver-completion.md` | consumes | `W_UNDERDETERMINED` (§3.6) and the objective-conflict/legibility surface. That PRD's `## DESIGN FORKS FOR LEO` remain unratified and are **not** resolved here. | that PRD | live |
| `v0_6/whole-model-objective-coupling.md` | consumes | Its ruling **"CP-SAT outer enumeration wrapping the continuous inner solve; MINLP rejected — do not re-litigate"** is honoured: this PRD replaces the *inner* continuous solve only. `WHOLE_MODEL_CLUSTER_DIM_CAP = 12` and `W_COUPLING_APPROXIMATED` are unchanged. | that PRD | live |
| `v0_6/discrete-cost-minimisation.md` PRD2 (#5469, #5472) | consumes | CP-SAT stays the discrete backend; leaf ν's capability surface is what #5469's registry wiring routes against. No leaf here touches `cpsat.rs`. | PRD2 | #5469 dispatchable |
| `v0_6/declared-intent-consumption-accounting.md` (#5415–#5421) | sibling | **#5416** owns the *pre-dispatch* `supports_auto_kind` envelope (an auto's **type** is unrepresentable); leaf δ owns the *in-dispatch* lowering refusal (a **constraint** is unlowerable). Complementary; align vocabulary, do not merge. **#5415** owns the zero-auto relate verification arm. | DIC | all pending |
| `docs/reify-language-spec.md` §10.3 | corrects | The spec's orchestrator framing survives; what changes is that its **Cross-domain** row ("span multiple domains simultaneously") becomes implementable — the classifier *partitions*, it cannot span. Leaf π carries the amendment. | this PRD | — |
| **#6251** non-negative kernel distance | consumes | Hard dependency of leaf ο (§6.4). | #6251 | pending |
| **#6572** guarded-group constraints never reach the solver | none new | A separate **intake** gap (no path reads `template.guarded_groups[*].constraints`). Named so a reader does not expect this PRD to close it. | #6572 | pending |
| **#5617** instance-path spelling of a solver-resolved auto | none new | Same template↔instance key schism as #6631/#6657, value plane. Sibling, no edge. | #5617 | pending |

**No new contested-ownership pair is introduced.** The three known contested seams (`persistent-naming-v2 ↔ multi-kernel`, `imported-field-source ↔ multi-kernel`, `topology-selectors ↔ persistent-naming-v2`) are untouched.

---

## 7. Contract section (B+H)

### 7.1 The lowering table — `LoweringTable` (the owned spec)

**Index.** A row is keyed by `(RelationKind, DatumKind, DatumKind)` where `DatumKind ∈ {Direction, Point, Axis, Plane, Frame}` — the locked lattice of `docs/design/geometric-relations.md` §2, no additions. Metric-bearing relations carry a third `Scalar` operand outside the key.

**Attributes, not kinds.** Surface kinds enter as a typed attribute record travelling with the datum:

```
DatumAttrs = { radius: Option<Length>, semi_angle: Option<Angle>, apex: Option<Point>, minor_radius: Option<Length>,
               form: SurfaceForm }
```

`SurfaceForm` **is** `reify_ir::FaceSurfaceKind` / `EdgeCurveKind` — the vocabularies are unified rather than a seventh enum added, and `Torus` (already named there, absent from the analytic extractor) becomes a representable, refusable form instead of an opaque throw. A row may *require* attributes: `tangent(Axis, Plane)` requires `radius` on operand 0, and its residual is `distance(axis, plane) − radius`. A required-but-absent attribute is a **runtime** refusal (§7.5 tier 2) naming the attribute and the surface form.

**Row contents — one row states every fact about the pair, once:**

```
Row = {
    codim:      u32,                       // the ONLY statement of ΔDOF for this pair
    kinds:      Option<(u32 /*rot*/, u32 /*trans*/)>,   // None where the split is genuinely undecidable
    requires:   AttrRequirement,
    arm:        Lowering,
    rows:       ResidualSpec,              // per-row DIMENSION + generator
}

Lowering = Analytic(ResidualGen)
         | KernelQuery { query: reify_ir::GeometryQuery, cost: CostClass }
         | Unavailable { reason: RefusalReason }
         | Ambiguous  { suggestions: &'static [&'static str] }
```

Three properties are contractual:

1. **Totality.** The table is exhaustive over `RelationKind × DatumKind × DatumKind` by construction — a non-exhaustive `match` fails to compile, and there is no `_ =>` arm producing an empty residual. Every one of the seven silent-zero paths of §2.4 resolves to `Unavailable{reason}` or `Ambiguous{suggestions}`.
2. **One codimension.** `relation_delta_dof` and `relation_delta_dof_kinds` are **derived from `Row.codim`/`Row.kinds`**, not maintained beside them. The dead `nominal_delta_dof` cross-check is deleted along with the divergence it was meant to catch: divergence becomes unrepresentable. (`one-fact-one-home`.)
3. **Dimensioned rows.** `ResidualSpec` declares each row's physical dimension. This is what §7.3's scaling consumes, and it is why the mixed rows of `axis_coincidence_residual` (2 dimensionless + 2 metre) and `frame_coincidence_residual` (3 metre + 3 radian) stop being compared against one metre-flavoured scalar.

**Return shape.** `Resolved | Unavailable{reason} | Ambiguous{suggestions}` with a redirect hint, adopted verbatim from `datum_projection_result_type` — the one existing table that already gets this right (`plane.dir` → `Unavailable` + hint `".normal"`).

**Validate before emit.** `LoweringTable::plan(relations, constraints) -> Result<LoweredProblem, Vec<Refusal>>` refuses the malformed set **whole**, before any residual row is built, in declaration order — `SketchSystem::validate`'s discipline, quoted at the head of this document, generalised.

**The judgment line (Leo's ruling, per-row).** `Analytic` for named design-intent carriers: axes, planes, spheres, cylinders, cones, frames, and a *single* named bounded face with a clamp. `KernelQuery` for trimming-dominated questions — nearest point possibly on an edge or vertex of a bounded face — and for anything at mesh-triangle granularity. Summing thousands of clamped-plane residuals is `BRepExtrema` re-implemented badly. The table makes this a per-row entry decision rather than an architectural argument.

### 7.2 The unified problem

```
Unknown  = PoseDof { sub: SubPath, component: u8 }      // 6 per `at auto` sub: 3 translation (m) + 3 exp-map (rad)
         | Scalar  { cell: ValueCellId }
         | Slack   { of: ConstraintNodeId }             // squared-slack, §7.4
         | Aux     { role: AuxRole }                    // e.g. the Chebyshev `t`

ResidualRow = { value: f64, dimension: DimensionVector, source: ResidualSource }
ResidualSource = Relation(RelationInstanceId) | Constraint(ConstraintNodeId) | Slack(ConstraintNodeId) | Aux(AuxRole)
```

**Multi-body from day one.** Today's state is a fixed `[f64; 6]` for exactly one body, and `solve_relate_scope` takes `scope.auto_unknowns.first()` — a scope with two `at auto` subs solves the first and silently leaves the second at identity. That is a defect this PRD closes (leaf θ), not a boundary it inherits.

**Pose parameterisation** stays the exponential map (minimal, no unit constraint, no renormalisation), with quaternion conversion confined to the value boundary as today. Leaf η adds the missing rotation-magnitude wrap so the state cannot drift unboundedly past π.

**`ResolutionProblem` widening.** The IR type carries `auto_params`, `constraints`, `current_values`, `objective`, `functions`, `dependent_cells` — no pose unknowns and no relations. It gains `pose_unknowns: Vec<FrameUnknown>`, `relations: Vec<RelationInstance>`, and `geometry: Option<&dyn GeometryQuery>` (§7.6). Existing construction sites are unaffected: empty vectors reproduce today's problem exactly, which is the byte-identity guarantee leaf ι's boundary tests pin.

**`dependent_cells` is consumed, not re-derived.** #5467 landed the single shared per-trial dependent-cell fold; the new residual evaluator uses it. No fourth fold site (the three existing ones — `eval_objective_set`, `eval_rank_cost`, `objective_term_contributions` — already carry `debug_assert!` backstops against exactly this).

### 7.3 Scaling — the conditioning contract

Every unknown declares a **nominal scale** `s_j` in its own dimension; every residual row declares a **nominal tolerance** `t_i` in the row's declared dimension. The solve runs in `x̃_j = x_j / s_j`, `r̃_i = r_i / t_i`.

| quantity | nominal value |
|---|---|
| pose translation | model characteristic length `L` (bounding extent of the participating datums), floored at `kernel_local` |
| pose rotation (exp-map component) | `1 rad` |
| scalar auto | box width if bounded, else `|seed|`, else 1 in its own SI unit |
| metre-dimensioned residual row | `solver_convergence` from the `RelateTolerance` ladder |
| dimensionless / radian residual row | the **scale-aware angular tolerance** `lin_tol / L` — `docs/design/geometric-relations.md` §2.3, not OCCT's `Precision::Angular` |

Consequences, each closing a measured defect: the convergence test becomes `max|r̃| ≤ 1`, dimensionally honest and unit-free, replacing an absolute `max_abs(r) ≤ tol` applied to a vector mixing metres, radians and pure numbers; the finite-difference step used at the kernel boundary becomes `ε·s_j` rather than one `FD_STEP = 1e-6` shared by metres and radians; and Levenberg's uniform `λI` — added today to a diagonal whose entries are metre² and radian² alike — becomes coherent.

**Single-sourcing.** The tolerance ladder is `kernel_local ≤ solver_convergence ≤ assertion/dedup` (design §2.3's coherence law), it is the same policy object #6653 introduces for verdicts, and it becomes **configurable** — `RelateTolerance` today has private fields, one constructor, no setters and no DSL knob.

**No automatic column equilibration.** Explicit dimensional scaling is interpretable and diagnosable; column-norm scaling would mask genuine geometric ill-conditioning (near-parallel axes) that the trust region handles and the diagnostics should *name*.

### 7.4 Inequalities and objectives — exactness over penalty

**Inequalities become equalities.** `g(x) ≥ 0` is lowered to the residual row `g(x) − s² = 0` with a fresh `Slack` unknown. The least-squares structure is preserved exactly, `PENALTY_WEIGHT` leaves the continuous core, no active-set combinatorics appear, and `s²` **is** the reported slack — the number P4's slack report wants and today has to re-derive. Cost is one unknown per inequality against clusters capped at 12 dimensions.

Honest weakness, stated: squared slack makes the constraint Jacobian rank-deficient at an active constraint (`s = 0`), degrading the local convergence *rate* there. The trust region handles it; the named upgrade when it bites is **augmented Lagrangian**, which also yields exact KKT multipliers (§9).

The robustness floor composes unchanged: `slack_i ≥ m` becomes `g(x) − m − s² = 0`. `E_ROBUSTNESS_FLOOR_INFEASIBLE` and `RobustnessFloorApplied` keep their meanings.

**Objectives leave the residual vector.** Driving an objective to zero is the wrong question, so the core solves *feasibility* and the objective is handled outside it:

1. Solve to feasibility with trust-region GN.
2. Householder-QR the scaled constraint Jacobian → orthonormal null-space basis `Z`.
3. Reduced gradient `Zᵀ∇f`, with `∇f` from the **same** dual-number AD.
4. Step along `−Z(Zᵀ∇f)`; restore feasibility with a GN correction; repeat.

This yields three things the present architecture cannot produce: an honest first-order optimality certificate `‖Zᵀ∇f‖` (a new `BestFoundReason::FirstOrderStationary`, replacing `Unreported`); **Lagrange multipliers** for active constraints, which are the sensitivity numbers P4 surfaces; and the elimination of the §2.6 defect at its root, because constraints are no longer purchasable.

**Chebyshev centrality is re-encoded, not re-implemented.** `Maximize(min_j slack_j)` folded as nested `Conditional`s is O(2ⁿ) and maximally non-smooth. The standard linear encoding — one `Aux` unknown `t`, rows `slack_j − t ≥ 0`, objective `maximize t` — is smooth, linear, and `n+1` rows. `build_centrality_objective`'s gate (all-Scalar autos, finite bounds, ≥1 slack) is unchanged; only the encoding changes.

**`eval_objective_set` stays the single objective fold** — P3 depends on CP-SAT's `solve_ranked` scoring through the same fold. The reduced-gradient loop calls it; it does not replace it.

### 7.5 Refusal — two tiers, derived from INV-SF-4

INV-SF-4 rules that *"A constraint that would be Indeterminate in every possible run (structurally unresolvable operand) violates INV-SF-3 and is a compile error"*, and classes *"a constraint that never entered the solver problem"* as an **unexpected** cause. That settles the tiering; it is not a fork.

**Tier 1 — static, compile error.** The `(relation, static operand kinds)` pair has no lowerable row. Knowable without running anything, therefore a coded `Error` at compile time naming the relation and both operand kinds, with the `Ambiguous` arm's suggestions where they exist. This is what turns §2.4's `All constraints satisfied.` into a diagnostic.

**Tier 2 — runtime, typed Indeterminate with an unexpected cause.** A required attribute absent because the surface is not analytic (`FaceSurfaceKind::BSplineSurface`); a degenerate operand; kernel budget exhausted. Genuinely run-dependent, so `Indeterminate` with an attributable reason — and *unexpected* under the doctrine, so plain `reify check` fails on it.

**Containment.** A refusal is scoped to its constraint. Sibling components keep their solved values. This replaces the early `return` inside `SolverRegistry::solve_inner`'s component loop that discards `merged_values` wholesale.

**Every diagnostic carries a code.** `reify-constraints` emits **zero** `W_`/`E_` mnemonics of its own today, and the single most common failure — an unrecognised pattern — surfaces as an uncoded `Diagnostic::warning` with no span and no constraint id, leaving every auto in the model `Undef` (INV-SF-6). New codes, all span-bearing:

| code | meaning |
|---|---|
| `E_RELATION_NOT_LOWERABLE` | tier-1 static table miss; names relation + both operand kinds |
| `E_RELATION_OPERAND_AMBIGUOUS` | `Ambiguous` arm; carries the suggestion list |
| `W_DATUM_ATTRIBUTE_UNAVAILABLE` | tier-2; names the attribute and the surface form |
| `E_SOLVER_BUDGET_EXHAUSTED` | iteration or kernel-query budget; carries counts and the step-norm trend |
| `E_SOLVER_INFEASIBLE` | converged to a non-zero minimum; carries `‖Jᵀr‖` and per-row blame |
| `W_SOLVER_UNDERDETERMINED_RANK` | rank deficiency; carries the null-space twists (feeds #4388) |
| `W_SOLVER_NONSMOOTH_STALL` | kink chatter; names the kink node's span |
| `E_SOLVER_DISJUNCTION_UNSUPPORTED` | top-level `Or` in a continuous constraint; names P3 as the owner |

**Cause attribution replaces conflation.** Today `solve_frame` emits only `Solved | Infeasible`, and iteration-limit exhaustion, damping-ceiling breakout and genuine geometric inconsistency all produce the same hand-written sentence; the `NoProgress` arm in `reify-eval` is annotated *"defensive"* and is dead. The carrier variants stay for compatibility; what changes is that each carries a typed cause distinguishing at minimum: converged-to-non-zero-minimum (the only honest "infeasible", certified by `‖Jᵀr‖`), budget-exhausted, rank-singular, lowering-refused, kernel-budget-exhausted, and non-smooth-stalled.

### 7.6 The kernel-query arm (O2)

**Dependency inversion is forced, not chosen.** `reify-constraints` depends on `reify-core`, `reify-ir`, `reify-expr`, `reify-stdlib`, `argmin`, `argmin-math`, `ndarray`, `tracing` — no kernel crate, no `reify-geometry` — and `reify-eval` depends *on* `reify-constraints`, so there is no back edge. The arm must therefore be a trait in `reify-ir`, implemented in `reify-eval`, consumed by `reify-constraints`. That is exactly `ComputeDispatch`'s shape (#4880), which is the ruled precedent.

```
pub trait GeometryDispatch: Send + Sync {
    fn query(&self, q: &reify_ir::GeometryQuery, poses: &[Pose], budget: &mut QueryBudget)
        -> GeometryDispatchOutcome;
}

pub enum GeometryDispatchOutcome {
    Value(Value),
    Refused(RefusalReason),      // unsupported query, non-analytic operand, kernel absent
    BudgetExhausted { spent: u32, budget: u32 },
}
```

**The query vocabulary is not invented here.** `reify_ir::GeometryQuery` already exists as the kernel-query enum — `Volume`, `SurfaceArea`, `Centroid`, `BoundingBox`, `MinDistance`, `FaceAnalyticDatum`, … — carrying `GeometryHandleId`s and already classified by `QueryCapability` (so a mesh or voxel kernel's inability to answer a `BRepOnly` query is already expressible). The trait takes that enum plus the trial poses; it adds dispatch, budget and a typed outcome, and nothing else. The trait is therefore named `GeometryDispatch`, in parallel with `ComputeDispatch` and to avoid colliding with the enum.

**It returns a typed outcome, never an `Option`.** `ComputeDispatch::dispatch -> Option<Value>` conflates *unregistered*, *failed* and *cancelled*, and callers "fall back to ordinary body evaluation in either case" — a silent fail-soft this PRD must not copy.

**Measured at decompose, and it changes the dependency set.** The D3 substrate-verification run established that `distance(a, b)` over two **cross-sub** geometry handles does not resolve on the build path at all today — the inline form reports `operator undefined for these operand kinds: StructureInstance` (the operand arrives as a `StructureInstance`, not a `GeometryHandle`), the `let`-bound form leaves the cell undef while still writing the STEP, and the `at auto` pose stays at identity under `>= 5mm`, `>= 500mm` and no constraint alike. So the query is not "verdict-only" here; it is **fully indeterminate**, because the sub's `at` placement is dropped before the query runs. That is **#6583** verbatim, and it is a **hard prerequisite** of this arm: §7.6's trait takes `poses` precisely because a query over unposed operands measures the wrong assembly. A second measurement from the same run, recorded because it sharpens §5's seam: the arity-2 `min_clearance(a, b)` overload is not merely absent, it is **silently accepted** — `reify check` exits 0 with `INDETERMINATE`, never a rejection.

**Scope: arm A only.** Queries over already-realized bodies whose **pose** varies. The arm is keyed on the **`reify_ir::GeometryQuery` variant**, not on a builtin's name — so `distance` (which exists today in `GEOMETRY_QUERY_NAMES` and is probe-confirmed to evaluate on the build path) is what leaf ο stands on, and the belt PRD's arity-2 `min_clearance` / `self_clearance` / `no_undeclared_interference` overloads become drivable **for free** when #5441–#5443 land, with no further work here and no dependency edge either way. Note the arity-3 *kinematic* `min_clearance(snapshot, a, b)` in `KINEMATIC_QUERY_NAMES` is a different function and is not the belt overload. Re-posing a realized body is a transform, not a re-realization. Per-trial geometry *re-realization* (a scalar auto feeding a geometry op's parameter — `geo_loop_a`/`geo_loop_b`, and M-WASTE's regime) stays **out of scope** with a named successor (§9); its existing decline keeps working and gains a better cause. The current decline message is already the best diagnostic in the corpus and is preserved: `error: unresolved constraint: … transitively depends on auto parameter(s) through geometry-backed inputs`.

**Budget.** Query count per solve plus a wall-clock guard; derivatives at this boundary are finite-difference with step `ε·s_j`, costing `n+1` queries per Jacobian column block, which is what the budget accounts for. Exhaustion is **loud** (`E_SOLVER_BUDGET_EXHAUSTED`), never a degraded silent answer.

**G6 hazard — the non-negative distance primitive (#6251).** `distance`, `min_clearance`, `intersects`, `interferes_with` and `interferes` all bottom out in one non-negative `BRepExtrema_DistShapeShape`. A non-negative distance carries **no gradient information about penetration depth**, so a clearance residual built on it cannot push two bodies apart from *inside* — the Jacobian is identically zero throughout the infeasible region, which is precisely where a solver needs it. Ruling: clearance rows require a **signed** distance. Until #6251 lands, a clearance residual whose current value is zero (contact or penetration) emits a tier-2 refusal naming the pair, rather than stalling on a zero gradient and reporting a converged wrong answer. Leaf ο depends on #6251.

### 7.7 The numeric core

**Derivatives, three sources, all declared in the table row.** Analytic Jacobian blocks for `Analytic` rows (dot products, norms and cross products of transformed datums — exact and hand-differentiable); **forward-mode dual numbers over `CompiledExpr`** for `constraint` residuals of arbitrary user algebra; finite differences **only** at the `KernelQuery` boundary. Forward mode is correct here because `n` is single-digit-to-low-tens (`WHOLE_MODEL_CLUSTER_DIM_CAP = 12` today), because Gauss–Newton needs the **Jacobian** rather than a gradient, and because it is one traversal carrying an `n`-vector per node rather than a tape. There is no autodiff anywhere in the workspace today.

**Step control: trust-region Gauss–Newton over Householder QR of the scaled Jacobian.**

- **QR, not normal equations.** `JᵀJ` squares the condition number; having just paid for careful scaling, throwing it away in the normal equations would be perverse. One factorisation yields the step, the **rank**, the **null-space basis**, and the driving-vs-redundant partition — four things computed today by two separate mechanisms (`normal_equations` + `solve6` for the step, a normalized Gram–Schmidt `add_rows_rank` for the rank). Collapsing them is `no-lockstep-duplication`, and the null space is what #4388's DOF ledger names geometrically.
- **Trust region, not uniform-λ Levenberg.** Today's loop adds a scalar `λ` to the raw diagonal with fixed ×0.5 accept / ×4 reject factors, no predicted-versus-actual gain ratio, no line search and no step cap. A ρ-ratio radius update is the standard, gives honest termination reasons, and is ~40 lines beyond what already exists.
- **Three convergence tests, not one.** Scaled residual `max|r̃|`, scaled step norm, and the **gradient norm `‖Jᵀr‖`**. The third is what separates "converged to a minimum that is not a root" (genuinely infeasible, with a certificate) from "ran out of budget" — the exact conflation §7.5 removes.

**Build, not adopt — reasoned.** `argmin` 0.10 is already a dependency and ships `gaussnewton_method`, `gaussnewton_linesearch`, `trustregion_method` (dogleg / Steihaug / Cauchy), `newton` and `quasinewton`; it ships **no** Levenberg–Marquardt. Its `TrustRegion` wants `Gradient` + `Hessian` (a general nonlinear program) rather than the least-squares structure, and its `TerminationReason` vocabulary cannot carry the causes of §7.5. Meanwhile a damped GN loop with the right accept/reject shape already exists in `gauss_newton_solve` and is test-pinned. Decision: **extend the in-tree loop**; keep `argmin`'s `NelderMead` solely as the loud non-smooth escape below.

**Kinks — active-branch (Clarke) Jacobian.** Non-smooth nodes are `Conditional`, `Match`, comparison `BinOp`s, `And`/`Or`/`Implies` Kleene folds, `min`/`max`/`abs`/`clamp`/`sign`/`floor`/`ceil`/`round`/`mod`, field reductions, and the `Undef` cliffs (division by zero, `sanitize_value`, argument short-circuit). The dual-number evaluator records the branch each kink node took; the Jacobian is the derivative of the **active** branch, a valid element of the Clarke subdifferential. Two safeguards: a **branch-change signature** difference between the accepted iterate and a trial contracts the trust region instead of accepting on cost alone; and alternation between two signatures more than `K` times emits `W_SOLVER_NONSMOOTH_STALL` naming the kink's span and falls back to the derivative-free path **for that component only, loudly**.

**Smoothing is rejected as a default.** An ε-smoothed `min` satisfies a *different* constraint from the one the author wrote, returning a well-typed wrong value — the INV-SF-7 failure shape. If ever needed it must be author-declared, never implicit.

**Disjunction is refused, not approximated.** A top-level `Or` is genuinely multimodal; `E_SOLVER_DISJUNCTION_UNSUPPORTED` names P3 as the owner. Today it silently becomes `min(violation_left, violation_right)`.

**Non-numeric operands stop contributing phantom gradients.** `comparison_violation` returns a flat `1.0` for a non-comparison or non-numeric operand and `10.0` for `Undef` — constants with no derivative that are nonetheless rendered to users as a measured *"max absolute residual: 1.00e0"*. These become tier-2 refusals: the constraint never entered the problem, which INV-SF-4 classes as an unexpected cause.

**Reconciling #5016.** `docs/prds/v0_6/whole-model-objective-coupling.md`'s δ ratified *Nelder-Mead + α-clustering + deterministic multistart*, rejecting MINLP and seeded global solvers. That ruling chose the **global** strategy — how many starts, how to cluster, how to rank. This PRD replaces the **local** solver underneath it. Multistart, clustering and ranking are untouched and continue to wrap a core that now converges with second-order-ish behaviour instead of a derivative-free simplex. Nelder-Mead survives in-tree as the non-smooth escape.

### 7.8 Sparsity — deferred, explicitly

Dense `Vec<Vec<f64>>` and dense Householder QR. Clusters are capped at 12 dimensions by `WHOLE_MODEL_CLUSTER_DIM_CAP`, and `decompose_into_components` splits further; single-digit-to-low-tens is the measured regime. Named successor when a *measured* cluster exceeds ~100 unknowns (§9). Stating this prevents a well-meaning implementer from importing a sparse stack for a 12×12 problem.

---

## 8. Boundary-test sketch (B+H) — facing both sides of each seam

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| **B1** | Unlowerable relation refuses statically | `tests/prd-gate/fixtures/solver_unification_tangent_silent_accept.ri` | `reify check` exits ≠ 0 with `E_RELATION_NOT_LOWERABLE` naming relation + both kinds + span. Today: `All constraints satisfied.`, exit 0, empty stderr. |
| **B2** | No vacuous success | any relation set whose lowered residual row count is 0 | the solve **refuses**; it never returns `Solved{unique:false}` at the seed. Today `r.is_empty()` returns success at identity. |
| **B3** | Table totality | property test over `RelationKind × DatumKind × DatumKind` | every triple resolves to exactly one arm; no triple yields an empty residual with no diagnostic. |
| **B4** | One codimension | property test | for every row, the codimension the compiler publishes and the codimension the solver uses are the *same field*; measured Jacobian rank ≤ `codim`, with a diagnostic when strictly less. |
| **B5** | Tangent solves | two named cylinders, one `at auto` | pose places them tangent to the scaled tolerance; STEP assertion on the centre distance = r₁+r₂. |
| **B6** | Per-constraint containment | one unlowerable constraint + one independently solvable component | the solvable component's autos resolve; the unlowerable one emits a coded, spanned diagnostic. Today every auto in the model is left `Undef`. |
| **B7** | Inequality + equality | `tests/prd-gate/fixtures/solver_unification_ineq_eq_penalty_offset.ri` (with its control `…_two_sided_control.ri`) | `w = 10mm`. Today: `could not be satisfied (max absolute residual: 5.00e-7)`. |
| **B8** | Inequality alone is honest | `w >= 8mm` on a strict `auto` | not `10 m` at exit 0 — either a bounded centrality answer with a stated box, or `W_SOLVER_UNDERDETERMINED_RANK`. *(Delta 2026-08-26, seam pass: for the no-objective one-sided STRICT-auto class, Leo's ruled P1-ε #6692 refusal — Error-class, fails plain check — is the only conformant outcome; the bounded-centrality and W_-warning arms must not be implemented for that class. Whether one-sided `auto(free)` also refuses or flows to the P3 §3.4 Partial-Warning row is an OPEN sub-case pending an operator ruling — check #6692/#6709 before implementing.)* |
| **B9** | Budget ≠ infeasible | a fixture that exhausts the iteration budget, and a genuinely infeasible fixture | two **different** codes: `E_SOLVER_BUDGET_EXHAUSTED` (with counts and step-norm trend) vs `E_SOLVER_INFEASIBLE` (with `‖Jᵀr‖` and per-row blame). |
| **B10** | Multi-body | two `at auto` subs in one scope | both placed; STEP shows both. Today the second is silently identity. |
| **B11** | Joint pose ⊕ scalar | `relate { distance(a, b, gap) }` — the **arity-3 DRIVE form** of the shared verb, which is the real shipped spelling (`is_relation_shared_verb`; `at_distance` is belt-PRD proposed naming and does **not** exist) — with `gap : Length = auto` | the pose and the scalar resolve in one solve. Impossible today: the relation drives the pose through `relate_solve`'s build-time pre-pass while the scalar is owned by `DimensionalSolver` behind the registry, and neither sees the other's unknown. |
| **B12** | Routing is `let`-invariant | `constraint distance(a,b) >= g` inline vs behind a `let` | identical resolved values and identical diagnostics. |
| **B13** | `antiparallel` means antiparallel | `relate { antiparallel(a.dir, b.dir) }` | directions oppose. Today lowered as `SLVS_C_PARALLEL`. |
| **B14** | Sketch path untouched | the `solve_sketch` corpus | byte-identical results before and after the expression-path retirement. |
| **B15** | Scaling is dimensionally honest | a model with metre-scale and radian-scale unknowns and a mixed-dimension relation (e.g. `axis_coincidence`'s 2 dimensionless + 2 metre rows), instantiated at two **physical scales** three orders of magnitude apart | the convergence verdict and the iteration count are invariant under the physical rescaling. Note the invariant is over physical *scale*, not authoring *unit*: `10mm` and `0.01m` compile to the same `si_value`, so a mm-vs-m test would be vacuous. Today the absolute `max_abs(r) <= tol` test fails this: the same geometry a thousand times larger meets a thousand-times-tighter relative bar. |
| **B16** | Kernel query drives | `constraint distance(a, b) >= 5mm` over **cross-sub** operands with one `at auto` sub, under `reify build` (geometry-consumer builtins resolve on the build/tessellate path, not on pure value-eval), with #6583 landed | the pose separates to satisfy the bound and the exported STEP shows it. Today (D3-measured) the constraint does not even reach a verdict on cross-sub operands: `operator undefined for these operand kinds: StructureInstance` inline, an undef cell when `let`-bound, and an identity pose under `>= 5mm`, `>= 500mm` and no constraint alike. Use a cross-sub fixture deliberately — a same-scope one would pass without demonstrating the capability the consumer needs. |
| **B17** | Kernel budget is loud | a model exceeding the query budget | `E_SOLVER_BUDGET_EXHAUSTED` with counts; never a silently degraded answer. |
| **B18** | Penetration refuses, not stalls | two interpenetrating bodies with a clearance residual, #6251 unlanded | tier-2 refusal naming the pair. Never a converged wrong answer on a zero gradient. |
| **B19** | Kink chatter escapes loudly | a deliberately chattering `clamp`-bearing residual | `W_SOLVER_NONSMOOTH_STALL` naming the span, then the derivative-free fallback — not a silent wrong value. |
| **B20** | Objective optimality is certified | `examples/continuous_cost_min.ri` | optimality reports `FirstOrderStationary` with `‖Zᵀ∇f‖`, not `Unreported`. |
| **B21** | Legacy problems are preserved — *not* byte-identically | a `ResolutionProblem` with empty `pose_unknowns` and empty `relations` | **problem construction** is identical (the widened struct with empty new fields builds the same problem), and every model in the existing solver corpus that resolved still resolves to the **same verdict and the same value within the shared tolerance policy**. Byte-identity is deliberately **not** asserted: replacing a derivative-free local solver with a derivative-based one changes trailing digits by construction, and reify's ratified position is that byte-identity is the wrong regime for an *iterative* result and the right one only for a *closed-form* result. Any corpus test that pins an iterative solver output to full precision is re-baselined to a tolerance in the leaf that moves it. |

---

## 9. Decomposition plan (G2 signal per task; task IDs assigned at decompose, 2026-08-26)

Phases: **1 = the table** (α–γ) · **2 = honest failure** (δ) · **3 = the numeric core** (ε–θ) · **4 = unified intake** (ι–μ) · **5 = routing + retirement** (ν–ξ) · **6 = kernel arm** (ο) · **7 = docs-truth + close** (π–τ).

**Task IDs** (assigned at decompose, 2026-08-26 — cite the ID, never a status word; the ID is immutable and queryable):
α #6668 · β #6669 · γ #6670 · δ #6671 · ε #6672 · ζ #6673 · η #6675 · θ #6676 · ι #6677 · κ #6678 · λ #6679 · μ #6680 · ν #6681 · ξ #6682 · ο #6683 · π #6684 · ρ #6685 · σ #6686 · τ #6687.
Out-of-batch hard edges wired as real dependencies: ζ ← #6653 · γ ← #5540 · ο ← #6251 · ο ← #6583. Coordinated without an edge: κ ↔ #6655 · δ ↔ #6659 · δ ↔ #5416 · η → #4388. Executed at decompose: **#6523 cancelled into ν #6681** (content ported first; zero dependents, verified) and **#5588 rescoped to item 2**, now depending on α #6668.

- **α #6668 — attribute-carrying datums** (§7.1 *Attributes*). Modules: `reify-kernel-occt` (`analytic_surface_datum_to_value`, `analytic_curve_datum_to_value`, the Sphere zero-direction, the missing Torus arm), `reify-eval/feature_datum.rs` (`Datum`, `datum_from_value`), `reify-ir` (`FaceSurfaceKind` alignment). *Signal (intermediate → β, γ, ο):* a fixture projecting a datum from a **Torus** face reports `W_DATUM_ATTRIBUTE_UNAVAILABLE` naming the surface form, where today the kernel throws an opaque runtime error; and a round-trip test shows a cylindrical face's radius reaching `Operand`. *Prereqs:* none. **Subsumes #5588 item 1** — executed at decompose: #5588 was rescoped to item 2 and now depends on α, and α's declared extent was widened to the two `relate_solve.rs` files item 1 named (`Operand`, `is_datum`) so the subsumption is not `producer-extent-short`.
- **β #6669 — the `LoweringTable`, total, one-codimension** (§7.1). Modules: `reify-constraints` (new `lowering.rs`; `relate_solve.rs` residual dispatch), `reify-compiler/relation_signatures.rs` (ΔDOF derived from the table). *Signal (leaf):* **B1** — `reify check tests/prd-gate/fixtures/solver_unification_tangent_silent_accept.ri` exits ≠ 0 with `E_RELATION_NOT_LOWERABLE` naming the relation and both operand kinds; today that exact committed file prints `All constraints satisfied.` at exit 0 with empty stderr. Plus **B2**, **B3**, **B4**. *Prereqs:* α.
- **γ #6670 — tangent rows** (§7.1). Modules: `reify-constraints/lowering.rs`, fixtures. *Signal (leaf):* **B5**. *Prereqs:* β; **out-of-batch hard: #5540** — γ adopts #5540's operand-conditional per-combo table as row data and must not re-derive it.
- **δ #6671 — typed per-constraint refusal + coded diagnostics + containment** (§7.5). Modules: `reify-constraints/registry.rs`, `reify-core/diagnostics.rs`, `reify-eval` consumption sites. *Signal (leaf):* **B6**. *Prereqs:* β. **Coordinates #6659** (in-progress): adopt its diagnostic vocabulary; δ extends refusal from *pattern* to *capability* and adds containment. **Coordinates #5416**: #5416 owns the pre-dispatch auto-**kind** envelope; δ owns the in-dispatch **constraint** lowering refusal.
- **ε #6672 — forward-mode dual-number AD over `CompiledExpr`** (§7.7). Modules: `reify-expr` (dual evaluator), `reify-constraints`. *Signal (intermediate → η, μ):* on **smooth** expressions across the `CompiledExprKind` numeric vocabulary, dual-number Jacobian columns agree with central differences to **1e-6 relative** — the honest bound for a central-difference reference, whose own truncation-plus-cancellation error floor is around 1e-8 relative at a well-scaled step, so a tighter assertion would be testing the reference rather than the implementation. Non-smooth nodes are excluded from the agreement check by construction and are covered instead by the branch-signature record, which ε must emit for every kink variant (λ consumes it). *Prereqs:* none.
- **ζ #6673 — dimensional scaling layer** (§7.3). Modules: `reify-constraints`, `reify-core` (the shared tolerance policy). *Signal (intermediate → η):* **B15**. *Prereqs:* none intra-batch; **out-of-batch hard: #6653** — ζ consumes its policy object rather than minting a second epsilon.
- **η #6675 — trust-region GN over Householder QR, three convergence tests, typed causes** (§7.7). Modules: `reify-constraints` (`relate_solve.rs` core, new `linalg.rs`), `reify-ir` (cause enum). *Signal (leaf):* **B9**. Also deletes the duplicate Gram–Schmidt rank pass and publishes the null-space basis #4388 consumes. *Prereqs:* ε, ζ.
- **θ #6676 — multi-body pose vector** (§7.2). Modules: `reify-constraints`, `reify-eval/relate_solve.rs` (`solve_relate_scope`'s `.first()`). *Signal (leaf):* **B10**. *Prereqs:* η.
- **ι #6677 — `UnifiedProblem`: one variable vector, one residual vector, relate-solve through the registry** (§7.2). **Integration gate.** Modules: `reify-ir/constraint.rs`, `reify-constraints/registry.rs`, `reify-eval/engine_build.rs` + `engine_eval.rs`. *Signal (leaf):* **B11** and **B21**. *Prereqs:* β, θ.
- **κ #6678 — squared-slack inequalities; retire `PENALTY_WEIGHT`; re-encode centrality** (§7.4). Modules: `reify-constraints/solver.rs`. *Signal (leaf):* **B7** — `tests/prd-gate/fixtures/solver_unification_ineq_eq_penalty_offset.ri` resolves `w = 10mm`, while its control `tests/prd-gate/fixtures/solver_unification_ineq_eq_two_sided_control.ri` stays green — and **B8**. *Prereqs:* ι. **Out-of-batch soft: #6655** (the box ceiling in B8 is #6655's subject; κ closes the centrality-vs-penalty half).
- **λ #6679 — kinks: active-branch Jacobian, branch-change contraction, loud chatter escape; disjunction refused** (§7.7). Modules: `reify-constraints`, `reify-core/diagnostics.rs`. *Signal (leaf):* **B19**, plus a `clamp`-bearing constraint driving an auto to the kink point. *Prereqs:* η, ε.
- **μ #6680 — reduced-gradient objective loop, optimality certificate, multipliers** (§7.4). Modules: `reify-constraints/solver.rs` + `registry.rs`, `reify-ir/ranked.rs` (`BestFoundReason::FirstOrderStationary`). *Signal (leaf):* **B20**. Preserves `eval_objective_set` as the single fold. *Prereqs:* η, κ.
- **ν #6681 — capability-based routing** (§7.5, §3 P4). Modules: `reify-constraints/classifier.rs`, `registry.rs`, `decompose.rs`. *Signal (leaf):* **B12**. **Absorbs #6523**, which was CANCELLED into this leaf at decompose after its content was ported (`domain_of_auto`'s reach into `cpsat::can_enumerate`, the `domain_spec` delegation target, and the `PRECONDITION on the Logical slot` doc block that must be deleted as the completion marker). ν's declared files were widened to `registry.rs` + `cpsat.rs` accordingly. *Prereqs:* δ, ι.
- **ξ #6682 — retire the 3D SolveSpace expression path.** Modules: `reify-constraints/solvespace.rs` (delete `SolveSpaceSolver`, `GeometricPattern`, `recognize_pattern` and its helpers; **keep** `SystemBuilder`, `get_sketch_normal`, `add_sketch`, `emit_sketch_constraint`, `solve_sketch`, all of `slvs_sys.rs` and `sketch.rs`). *Signal (leaf):* **B13** and **B14**. *Prereqs:* ν, γ.
- **ο #6683 — the `GeometryQuery` arm: trait, budget, typed exhaustion, distance rows drivable** (§7.6). Modules: `reify-ir` (trait), `reify-eval` (impl over posed realized bodies), `reify-constraints` (`KernelQuery` arm). *Signal (leaf):* **B16**, **B17**, **B18**. *Prereqs:* α, ι, η; **out-of-batch hard: #6251 and #6583** (the latter added by the decompose-time D3 run — see §7.6). **Signal deliberately stands on `distance`, not `min_clearance`:** the capability-manifest walk caught that the arity-2 `min_clearance(a, b)` overload this leaf originally asserted does **not** exist on main — it is belt leaf #5441, which is pending, outside this batch, and not upstream of ο, i.e. a `producer-absent` binding. `distance` is registered in `GEOMETRY_QUERY_NAMES` today and probe-confirmed to evaluate on the build path, so ο's signal is delivered by its own dependency set. Because the arm dispatches on query kind, the belt overloads ride it for free once #5441–#5443 land.
- **π #6684 — doc-chunk update, registry-verified** (docs-truth gate leaf 1). Modules: `crates/reify-mcp/src/tools/chunks/constraints.md` (+ `geometry.md` where signatures move). *Signal (leaf):* the ten relation names — none of which appears in **any** of the 17 chunks today — are documented with the `relate`-vs-`constraint` rule, the drivable-clearance idiom, and a corrected *Constraint Status* section (its present text ties `Indeterminate` to undef inputs only, narrower than INV-SF-4's doctrine); the `constraint def Coaxial` example, which currently teaches hand-written predicate algebra as the way to express coaxiality, gains the `concentric` form. Each documented signature spot-verified against `RELATION_FN_NAMES` and the `units.rs` registries in the task's own diff. Also carries the `docs/reify-language-spec.md` §10.3 amendment (§6) and the `constrained-2d-sketch.md` §11 correction. *Prereqs:* γ, κ, ο. **Coordinates belt ν #5446, sketch θ #5513, #5389 (in-progress), #5347** — no duplicated content. **Note:** `reify-audit --pattern PDOCCOVER` cannot catch this omission — its census reads only `units.rs`, and `RELATION_FN_NAMES` lives in `relation_signatures.rs`.
- **ρ #6685 — exemplar corpus** (docs-truth gate leaf 2). Modules: `examples/best_practices/<name>.ri` + its `INDEX.md` row, **same commit** (`examples_smoke.rs::best_practices_index_matches_corpus_directory` pins the bidirectional invariant). *Signal (leaf):* a worked exemplar contrasting the named-relation idiom against the hand-algebra anti-pattern, compiling clean and with every constraint `Satisfied` under `best_practices_constraint_gate.rs` (#6215). *Prereqs:* γ, κ, ο.
- **σ #6686 — cheatsheet index + discoverability acceptance** (docs-truth gate leaves 3–4). Modules: `.claude/skills/reify-design/SKILL.md`. *Signal (leaf):* a one-line index entry pointing at ρ's corpus file, plus a committed transcript showing that intent-level queries ("make these two parts touch", "keep this clear of that", "let the solver place this") reach the mechanism from the chunk topics or the corpus index without knowing a feature name. *Prereqs:* π, ρ.
- **τ #6687 — PRD close.** *Signal (leaf):* this document's Status marker set to the terminal token with the landed leaf IDs, the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, and the matching header on the capability manifest. *Prereqs:* every other leaf (real `add_dependency` edges).
**Companion correction edits**, carried same-diff by the leaf that already touches each file rather than filed separately: `docs/prds/v0_6/constrained-2d-sketch.md` §11's "Solver consolidation" bullet gains the O3-scoped refinement — the sketch path is kept — (leaf π); `crates/reify-constraints/src/solvespace.rs`'s matching doc comment goes with the code it annotates (leaf ξ). **#5588 was rescoped to item 2 only and #6523 was cancelled into ν, both executed at decompose 2026-08-26** rather than left as obligations on a future steward — a pending task is dispatchable, and "close it as absorbed later" is the bookkeeping this document's own terminal-status discipline exists to stop relying on.

**DAG:** α→{β,γ,ο} · β→{γ,δ,ι} · γ→{ξ,π,ρ} · δ→ν · {ε,ζ}→η · η→{θ,λ,μ,ο} · θ→ι · ι→{κ,ν,ο} · κ→{μ,π,ρ} · ν→ξ · ο→{π,ρ} · {π,ρ}→σ · all→τ.
**Out-of-batch hard edges:** ζ←#6653 · γ←#5540 · ο←#6251. **Soft/coordination:** κ↔#6655 · δ↔#6659 · δ↔#5416 · η→#4388 (produces the null space; no edge). **Executed at decompose:** #6523 cancelled into ν; #5588 rescoped to item 2 and dep-wired onto α.

**G7 walk** against `docs/legibility/design-invariants.md` (INV-SF-1..7, INV-AD-1..4), every task, not only leaves:

- `undef-has-provenance` (SF-1) — every refusal and every non-`Solved` cause records an `UndefCause`; δ is the leaf that makes this true where §2.4's silent-zero paths left no record at all.
- `error-severity-exits-nonzero` (SF-2) — new codes are `Error` only where a healthy path cannot hit them; tier-2 attribute-unavailable is `Warning` + Indeterminate-with-cause, which plain `check` fails on under the INV-SF-4 doctrine rather than via a per-code escalation list.
- `declared-intent-consumed-or-diagnosed` (SF-3) — the whole point of §7.1's totality: a relation that removes no DOF is diagnosed, never absorbed as "redundant".
- `indeterminate-attributable-transient` (SF-4) — §7.5's two tiers are derived from this invariant; the static tier is a compile error precisely because it would be indeterminate in every run.
- `placeholders-owned-and-loud` (SF-5) — no placeholder types introduced; `Datum::Axis.radius`'s existing `#[allow(dead_code)]` dead field is *removed* by α rather than extended.
- `diagnostics-carry-codes` (SF-6) — the table in §7.5 exists because `reify-constraints` emits zero mnemonics today; δ is the leaf that closes it.
- `parse-is-value-faithful` (SF-7) — no grammar added. The invariant's *spirit* (a well-typed wrong value is the worst shape) is why §7.7 rejects implicit kink smoothing and why B2 forbids vacuous success.
- **`angle-crossings-explicit` (AD-1)** — the relation vocabulary carries `Angle`-typed metric operands. Every such operand's `rad` arrives through the existing dimensional system's named angle units; **no relation residual manufactures `rad` from a quotient**. The `angle` row's residual is `dot(û_a, û_b) − cos θ` — dimensionless, with `θ` arriving already `Angle`-typed.
- **`boundaries-declare-angle-convention` (AD-4)** — the retiring SolveSpace expression path's `angle_deg` seam is an undeclared boundary today. ξ **deletes** that boundary rather than documenting it; the surviving `add_sketch` seam's convention is `constrained-2d-sketch`'s to declare, and the angle-dimension-completion leaves ι/υ own it. Named here so it is not assumed correct by silence.
- `no-lockstep-duplication` — three consolidations are load-bearing, not incidental: one codimension (β), one QR replacing step+rank (η), one objective fold preserved (μ).

**No waivers required.**

**Gate-test drift-guard registration.** New tests are ordinary crate tests plus new `.ri` fixtures under `tests/prd-gate/fixtures/`. No new `tests/infra/test_*.sh`, no new gate-resident standalone binary, no wall-clock assertions. Any leaf whose fixture becomes an input to a compiled Rust test target carries its `_RUST_COUPLED_RI_FIXTURES` entry (and, if grammar-pinned, its `EXPECTED_CLEAN` + `_GUI_COUPLED_RI_FIXTURES` entries) **in its own diff** — `tests/infra/test_verify_scope.sh`'s PG-DRIFT re-derives the coupled set from `git grep` on every infra run and goes red on an unregistered reference.

---

## 10. Out of scope for this PRD (named successors)

- **Basin enumeration, multistart policy, global uniqueness, discrete×continuous coupling** → the **P3** PRD, which consumes this core. `#5388` and `#5711` live there; `#5472`/`#5474` are PRD2's own leaves ζ/θ (discrete-cost-minimisation), which P3 sequences around rather than owning. *(Corrected 2026-08-26 per P3's §7.1 companion obligation — the original line mis-assigned PRD2's leaves.)*
- **Telemetry and presentation** (GUI badges, reify-debug MCP tools, `reify explain` parity, slack rendering, the DOF badge) → the **P4** PRD. This PRD *produces* the causes, multipliers and slacks; it renders none of them.
- **Which commands solve** (`cmd_build` / `cmd_check` posture, determinism across cold/warm/edit, cost tiers, LSP and `mcp-server` postures) → the **P1** PRD.
- **Per-trial geometry re-realization** (a scalar auto feeding a geometry op's parameter; `geo_loop_a`/`geo_loop_b`; M-WASTE's regime; design §12's bounded fixpoint). The existing typed decline stands and gains a better cause. Successor: an outer candidate-sweep PRD gated on the realization-cache input-cone rekey.
- **Sparse linear algebra.** Named successor when a measured cluster exceeds ~100 unknowns.
- **Augmented Lagrangian.** The named upgrade path from squared slack, warranted when active-set degeneracy is *measured* or when exact KKT multipliers are needed beyond what the reduced-gradient loop supplies.
- **2D sketch consolidation.** Explicitly retained on libslvs per O3-scoped. Honest note: `solve_sketch`, `SketchSystem` and `SketchSolveResult` have **no production consumer** outside their own crate and tests — keeping the sketch path keeps a landed, well-built, span-attributing, 18-constraint substrate that is *awaiting* a language-level consumer (`constrained-2d-sketch` α, γ–θ, all pending). That is a correct decision, and it should not be read as preserving live user-facing behaviour.
- **First-class `Circle`/`Sphere`/`Cylinder`/`Cone` value types** and the `<HasAxis & HasRadius>` operand form (#5588 item 2, rescoped at decompose). §7.1 carries attributes on datums precisely so the table does not wait on it. Note the decompose-time correction: that form is **not** blocked on call-site trait conformance — that landed (#2227, #4081) and is wired — and multi-supertrait refinement already ships (`trait Watertight : Closed + Manifold {}`), so the open question there is nominal-vs-structural spelling, not a missing type-system capability.
- **`ObjectiveCombination::Lexicographic`, `ObjectiveTerm.weight ≠ 1.0`, `priority ≠ 0`, `AutoParam.bounds = Some(..)`** — four pieces of solver-facing surface unreachable from `.ri` source today. Making them reachable is `constraint-solver-completion`'s territory; this PRD neither uses nor removes them.
- **#6572** guarded-group constraint intake, **#5617** instance-path value plane, **#6583/#6586/#6592** the cross-sub realization defects. Named in §4/§6 as consumer gates, owned elsewhere.

---

## 11. Open questions (tactical — decided at implementation time)

1. **Trust-region radius update constants** (initial radius, expand/contract factors, ρ acceptance thresholds). Standard values (ρ > 0.75 expand ×2, ρ < 0.25 contract ×0.25, accept above 0.1) are a fine default. Decide during η against the existing solver corpus.
2. **Kink chatter threshold `K`.** Suggested: 3 alternations of the same two branch signatures. Decide during λ.
3. **Kernel-query budget default and its knob surface** (per-solve query count; whether it is a config key, a per-model pragma, or both). Suggested: a config default with a `--solver-query-budget` override; no `.ri` surface in v1. Decide during ο.
4. **Characteristic length `L`.** Bounding extent of the participating datums vs. the scope's own bounding box vs. a per-model configured value. Suggested: participating-datum extent, floored at `kernel_local`, recomputed per solve. Decide during ζ.
5. **Whether `E_RELATION_NOT_LOWERABLE` should be downgraded to a warning behind an explicit author opt-in** for exploratory models. Suggested: no opt-in in v1 — the whole point is that silence was the defect. Revisit only with a measured request.
6. **Dual-number width.** A fixed-width dual (compile-time `N`) vs. a `Vec`-backed one. Suggested: `Vec`-backed, since cluster dimension is dynamic and ≤ 12 today. Decide during ε.
7. **Whether `individual_rank`** (computed today, read by nothing) is deleted or wired to the per-relation codimension cross-check in β. Suggested: wired — it becomes the measured half of B4.
