# Capability manifest — geometry⇄algebra solver unification

PRD: `docs/prds/v0_6/geometry-algebra-solver-unification.md` · Built at decompose, 2026-08-26 · **Code anchors** verified against main `2ea72cc5e8` (2026-08-26); cite-by-symbol, re-locate lines at implementation time.

Mechanizes G3 + G6 per leaf. Any binding resolving to `declared-only` / `test-only` / `producer-absent` / `producer-downstream` / `producer-extent-short` / `fixture-ERROR` / `bound≤floor` / `rejection-absent` blocks the batch.

**One binding was resolved during this walk** — it is recorded in full at ο below, because it is the manifest doing its job rather than a formality: leaf ο originally asserted `constraint min_clearance(a, b) >= 5mm`. `min_clearance` **is** registered on main, but only as the **arity-3 kinematic** form `min_clearance(snapshot, a, b)` in `KINEMATIC_QUERY_NAMES`. The **arity-2** Structure/Geometry overload the signal assumed is belt leaf **#5441** — `pending`, outside this batch, and not upstream of ο: a textbook `producer-absent`. Resolved by re-homing ο's signal onto `distance`, which is registered in `GEOMETRY_QUERY_NAMES` and probe-confirmed to evaluate on the build path, and by keying the kernel arm on **query kind** rather than builtin name so the belt overloads ride it for free when #5441–#5443 land.

**Rejection-mechanism convention used throughout.** Most leaves assert that a *new* diagnostic fires. Those are `producer-self`: the leaf builds the diagnostic and its own fixture observes it fire. No leaf claims a rejection of today's substrate — with one deliberate exception, β, which asserts today's substrate **fails to reject** and commits the fixture proving it.

---

## α — attribute-carrying datums (intermediate → β, γ, ο)

| capability | binding | verdict |
|---|---|---|
| analytic surface/curve datum production | capability→producer, **wired on main**: `face_analytic_datum` / `edge_analytic_datum` (occt_wrapper) → `OcctKernel::query` arms → `analytic_surface_datum_to_value` / `analytic_curve_datum_to_value`. `AnalyticSurfaceDatum` already carries `scalar1`/`scalar2` across the FFI. | PASS |
| the radius is currently dropped (the defect α closes) | capability→producer, **producer-self**: three stacked losses, each greppable — `analytic_surface_datum_to_value` reads neither scalar; `datum_from_value` hard-codes `radius: None`; `Operand.datum: Value` admits no attribute channel. `Datum::Axis.radius` is `#[allow(dead_code)]` and never `Some` in tracked source. | PASS |
| `FaceSurfaceKind` / `EdgeCurveKind` exist to align against | capability→producer, **wired on main**: both in `reify-ir`, consumed by `selector_vocabulary_v2::faces_by_surface_kind` / `edges_by_curve_kind` for the `%Plane`/`%Cylinder` selector slots. Both already name `Torus`; the analytic extractor does not — α's alignment work. | PASS |
| Torus refusal is observable | producer-self. Today a torus face hits the extractor's `throw std::runtime_error("face_analytic_datum: non-analytic surface …")`; α converts it to `W_DATUM_ATTRIBUTE_UNAVAILABLE` naming the form. α delivers the diagnostic and its fixture observes it. | PASS |
| **subsumes #5588 item 1 — EXECUTED** | #5588 was **rescoped at decompose** to item 2 only and now depends on α (deps `[5540, 6668]`); no inversion (it depends on α, not the reverse). α's declared extent was widened to the two `relate_solve.rs` files item 1 named, so this is not `producer-extent-short`. Item 2's stated blocker was found **stale**: call-site trait conformance is live and wired (#2227, #4081 both `done`), and `trait Watertight : Closed + Manifold {}` already ships — so the `ty.rs` "deferred" comment is a docs-truth defect the rescoped task now owns. | PASS |

## β — the `LoweringTable`, total, one-codimension (leaf)

| capability | binding | verdict |
|---|---|---|
| **today's silent accept is real** (the RED half) | **rejection-check, OBSERVED**: `reify check tests/prd-gate/fixtures/solver_unification_tangent_silent_accept.ri` → exit 0, `All constraints satisfied.`, **empty stderr** (run 2026-08-26 against `target/debug/reify`). This is the negative-assertion mandate satisfied in its *inverted* form — the rejection capability is confirmed **ABSENT**, which is precisely the defect β closes. Fixture committed. | PASS |
| the four residual-dispatch sites to consolidate | capability→producer, **wired on main**: `residual_dispatch`, `coincident_residual`, `distance_residual`, `on_residual` (`relate_solve.rs`) — all on the production `solve_relate_scope` → `build_with_geometry_output` path. | PASS |
| the parallel ΔDOF table to fold in | capability→producer, **wired on main**: `relation_delta_dof`, `relation_delta_dof_kinds`, `relation_operand_datum` (`relation_signatures.rs`), `pub(crate)`, consumed by the compiler's relation type-check. | PASS |
| the datum lattice is fixed, not invented here | capability→producer: `docs/design/geometric-relations.md` §2 + decision log §14 (locked 2026-06-08). `Value::{Direction,Point,Axis,Plane,Frame}` all exist as first-class values with resolvable type names. | PASS |
| `E_RELATION_NOT_LOWERABLE` fires | producer-self — β delivers the code and the fixture above observes it flip from exit 0 to exit ≠ 0. | PASS |
| grammar | **N/A** — no novel syntax. The committed fixture parses and runs today. | PASS |

## γ — tangent rows (leaf)

| capability | binding | verdict |
|---|---|---|
| `tangent` is in the vocabulary and types as a Relation | capability→producer, **wired on main**: `RELATION_FN_NAMES` (10 names incl. `tangent`); `relation_fn_result_type` → `Type::Relation`; `relation_delta_dof("tangent", _) → Some(2)`. Probe-confirmed: a `tangent(Axis, Axis)` relate block type-checks and builds at exit 0 today. | PASS |
| the per-combo ΔDOF table | **producer:task-#5540 UPSTREAM** — hard `add_dependency` edge. #5540 (`pending`, high, no deps, dispatchable) carries the authoritative amended per-combo table (sphere/plane 1, sphere/sphere 1, cyl/cyl 1, cyl/plane **2**) and the `relation_delta_dof_kinds`-stays-`None` reviewer coupling note. γ **adopts** those rows; it does not re-derive them. DAG-direction verified: #5540 does not depend on any leaf here. | PASS |
| the radius reaches the operand | **producer:task-α UPSTREAM** (intra-batch edge γ←α). Without α a cylinder is indistinguishable from a line at `residual_dispatch` — this is exactly why the binding is an edge and not an assumption. | PASS |
| `.axis` projects from a cylinder | capability→producer, **wired on main**: `datum_projection_result_type` `Geometry → .axis : Axis`; probe-observed `BossB.boss_axis = axis(point(0.025 m, 0 m, 0 m), direction(0, 0, 1))`. | PASS |
| numeric floor | the tangency assertion is stated against the **scaled** tolerance from leaf ζ's ladder (`kernel_local ≤ solver_convergence ≤ assertion`), not a guessed absolute. No bare bound asserted. | PASS |

## δ — typed per-constraint refusal, coded diagnostics, containment (leaf)

| capability | binding | verdict |
|---|---|---|
| the whole-resolve kill is real | capability→producer, **wired on main**: `SolverRegistry::solve_inner`'s `NoProgress` arm returns from inside the component loop, discarding `merged_values`. | PASS |
| diagnostics are uncoded today | capability→producer, **greppable ABSENCE**: `reify-constraints` emits **zero** `W_`/`E_` mnemonics of its own; the `NoProgress` consumption sites build a bare `Diagnostic::warning` with no `.with_code`. No `DiagnosticCode` variant for solver no-progress exists in `reify-core`. INV-SF-6. | PASS |
| `DiagnosticCode` is extensible | capability→producer, **wired on main**: `reify-core/src/diagnostics.rs` carries `ConstraintViolated`, `ConstraintIndeterminate`, `ConstraintUnsatisfiable`, `ConstraintNonUnique`, `SolverOptimalityUnproven`, `RobustnessFloorInfeasible` — the pattern δ extends. | PASS |
| not duplicating #6659 | DAG-direction: #6659 is `in-progress` with a live claimant and ships the refusal **shape** with no capability change; its own description says this PRD "subsumes this trap properly via residual lowering". δ extends refusal from *pattern* to *capability* and adds containment. **No edge** — δ must not block on an in-flight interim. | PASS |
| not duplicating #5416 | Scope split recorded: #5416 owns the **pre-dispatch auto-kind** envelope (`supports_auto_kind`); δ owns the **in-dispatch constraint-lowering** refusal. #5416's own text names them complementary. | PASS |

## ε — forward-mode dual-number AD over `CompiledExpr` (intermediate → η, μ)

| capability | binding | verdict |
|---|---|---|
| there is no autodiff to collide with | capability→producer, **greppable ABSENCE**: no `autodiff`/`num-dual`/`Gradient`/`Jacobian` impl anywhere in `crates/`; the only `argmin::solver::` import in the workspace is `neldermead`. | PASS |
| `CompiledExpr` is a closed, traversable vocabulary | capability→producer, **wired on main**: `CompiledExprKind` (25 variants) + `eval_expr` (`reify-expr`), the single evaluation entry the solver already uses via `ctx_with`. | PASS |
| **numeric floor** — the 1e-6 relative FD-agreement bound | **floor stated and cleared**: central differences at a well-scaled step carry a truncation-plus-cancellation error floor of order **1e-8 relative**; the asserted agreement bound is **1e-6 > 1e-8**. A tighter bound would be testing the reference, not the implementation. Non-smooth nodes are excluded by construction and covered by the branch-signature record instead. | PASS |

## ζ — dimensional scaling layer (intermediate → η)

| capability | binding | verdict |
|---|---|---|
| dimensions are available per value | capability→producer, **wired on main**: `Value::Scalar { si_value, dimension }`, `DimensionVector` (10-basis incl. Angle and Money), `dimension_of`. Every solver value is SI base units by construction. | PASS |
| a tolerance **hierarchy** already exists to generalize | capability→producer, **wired on main**: `RelateTolerance::{kernel_local, solver_convergence, assertion}` — the only coherent tolerance ladder in the codebase. Private fields, one constructor, no setters: ζ makes it configurable. | PASS |
| single-sourcing with the verdict fold | **producer:task-#6653 UPSTREAM** — hard `add_dependency` edge. #6653 (`pending`, **high**) creates the shared relative-plus-dimension-aware-floor policy; ζ **consumes** it rather than minting a second epsilon. DAG-direction verified: #6653 has no dependencies and depends on no leaf here. | PASS |
| the scaling gap is real | capability→producer: `FD_STEP` is one constant for metres and radians; LM damping adds a scalar to a diagonal mixing m² and rad²; `FEASIBILITY_THRESHOLD = 1e-12` is a dimension-blind absolute on raw SI. All greppable in `relate_solve.rs` / `solver.rs`. | PASS |

## η — trust-region GN over Householder QR, typed causes (leaf)

| capability | binding | verdict |
|---|---|---|
| a damped GN loop exists to extend | capability→producer, **wired on main**: `gauss_newton_solve` (`relate_solve.rs`) — FD Jacobian, Levenberg accept/reject schedule, `normal_equations` + `solve6`, reached in production via `solve_frame` ← `solve_relate_scope` ← `build_with_geometry_output`. | PASS |
| the duplicate rank pass exists to fold in | capability→producer, **wired on main**: `add_rows_rank` (normalized Gram–Schmidt) serves `partition_driving_set` and `rank_of` **separately** from the step's `normal_equations`/`solve6`. One QR replaces both. | PASS |
| cause conflation is real | capability→producer: `solve_frame` emits only `Solved`/`Infeasible`; iteration-limit, λ-ceiling breakout and true inconsistency share one hand-written message; the caller's `NoProgress` arm is annotated *"defensive"* and is dead. | PASS |
| `‖Jᵀr‖` is computable | producer-self, **gated on ε** (intra-batch edge η←ε): a gradient-norm certificate requires a Jacobian, which is exactly what ε delivers. Recorded as an edge, not an assumption. | PASS |
| no argmin adoption premise | capability→producer, **verified in the vendored registry**: argmin 0.10 ships `gaussnewton_method`, `gaussnewton_linesearch`, `trustregion_method` (dogleg/Steihaug/Cauchy), `newton`, `quasinewton` — and **no** Levenberg–Marquardt. The PRD's build-not-adopt reasoning rests on that inventory, not on an assumption. | PASS |

## θ — multi-body pose vector (leaf)

| capability | binding | verdict |
|---|---|---|
| the single-body limit is real | capability→producer, **wired on main**: `const FRAME_DOF: usize = 6`, state is `[f64; 6]` (not a `Vec`), every linalg helper is hard-typed to it, and `solve_relate_scope` takes `scope.auto_unknowns.first()`. `grep` for a `Vec<FrameUnknown>` API returns nothing. | PASS |
| grounding already considers all auto subs | capability→producer, **wired on main**: `trace_to_ground` walks every auto unknown for the global-float check — so a second auto sub already passes grounding validation and is then never placed. That asymmetry is θ's RED. | PASS |
| **premise under D3 verification** | The "second sub silently at identity, no diagnostic" claim is source-derived; leaf `theta` was submitted to the D3 workflow for behavioural confirmation. | PASS (see §D3) |

## ι — `UnifiedProblem` integration gate (leaf)

| capability | binding | verdict |
|---|---|---|
| `ResolutionProblem` is widenable | capability→producer, **wired on main**: the struct + its two builders `build_solver_problem` / `build_merged_solver_problem`, plus two inline literals on the edit path. Empty new fields reproduce today's problem exactly. | PASS |
| the shared per-trial fold exists and must be reused | **producer:task-#5467 (done)** — landed the ONE shared dependent-cell fold. ι consumes it; no fourth fold site (the three existing sites already carry `debug_assert!` backstops). | PASS |
| relate-solve is reachable to re-route | capability→producer, **wired on main**: `solve_scopes` has exactly one production call site, inside `build_with_geometry_output`. A single seam to move. | PASS |
| **byte-identity is NOT asserted** | numeric-floor/regime check: reify's ratified position is that byte-identity is the wrong regime for an **iterative** result and right only for a **closed-form** one. ι asserts *problem-construction identity* + *same verdict and value within the shared tolerance policy*, never trailing-digit identity. Corpus tests pinning iterative output to full precision are re-baselined by the leaf that moves them. | PASS |

## κ — squared-slack inequalities, centrality re-encoded (leaf)

| capability | binding | verdict |
|---|---|---|
| **the defect is measured, derived, and controlled** | **rejection-check, OBSERVED + DERIVED**: `tests/prd-gate/fixtures/solver_unification_ineq_eq_penalty_offset.ri` → `error: constraints could not be satisfied (max absolute residual: 5.00e-7)`, exit 1. The value is *derived* from `cost(w) = −slack + PENALTY_WEIGHT·(w−target)²` ⇒ `w* − target = 1/(2·PENALTY_WEIGHT) = 5e-7`, and *discriminated* by the committed control `…_two_sided_control.ri` which solves cleanly because two opposing slack gradients cancel. Both fixtures committed. | PASS |
| the penalty machinery to retire | capability→producer, **wired on main**: `PENALTY_WEIGHT`, `comparison_violation`, `constraint_violation`, `ConstraintCostFunction::cost`. | PASS |
| slack collection already exists | capability→producer, **wired on main**: `collect_slack_terms`, `collect_floor_terms` — three inequality-derived structures sharing one op-rule pact. | PASS |
| the centrality encoding to replace | capability→producer, **wired on main**: `build_centrality_objective` folds `min` as nested `Conditional`s and warns above `CENTRALITY_SLACK_WARN_THRESHOLD = 10` about its own O(2ⁿ) growth. Its gate (all-Scalar autos, finite bounds, ≥1 slack) is unchanged by κ. | PASS |
| the `10 m` half is **not** claimed as κ's delivery | DAG-direction honesty: `w >= 8mm → 10 m` is the **ceiling of the dimension-default box** (`default_bounds_for` LENGTH → `(1e-6, 10.0)`) reached by maximising the sole slack. That box is **#6655**'s subject, not κ's. Recorded as a **soft** coordination, no edge, and B8 is phrased as "not `10 m` at exit 0" — satisfiable by κ alone via an honest underdetermined verdict. | PASS |
| robustness floor composes | capability→producer, **wired on main**: `synthesise_floor_constraints`, `objective_is_money`, `RobustnessFloorInfeasible`. `slack ≥ m` becomes `g − m − s² = 0`. | PASS |

## λ — kinks: active-branch Jacobian, chatter escape (leaf)

| capability | binding | verdict |
|---|---|---|
| the kink inventory is enumerable | capability→producer, **wired on main**: `Conditional`, `Match`, comparison `BinOp`s, Kleene `And`/`Or`/`Implies`; stdlib `min`/`max`/`abs`/`clamp`/`sign`/`floor`/`ceil`/`round`/`mod`; field reductions; the `Undef` cliffs (`eval_div` zero-denominator, `sanitize_value`, argument short-circuit). | PASS |
| kinks already work derivative-free | capability→producer, probe-confirmed: `expr_min.ri`, `expr_if.ri`, `expr_abs.ri`, `expr_sqrt.ri` all solve today. λ must not regress them — that is the floor it is measured against. | PASS |
| the branch-signature record | **producer:task-ε UPSTREAM** (intra-batch edge λ←ε). ε's signal explicitly includes emitting the record for every kink variant, so λ's premise is delivered by its own dependency set. | PASS |
| `W_SOLVER_NONSMOOTH_STALL` fires | producer-self — λ delivers the code and its chattering fixture observes it. | PASS |
| disjunction refusal | producer-self. Today `Or` silently becomes `min(violation_left, violation_right)` (greppable in `constraint_violation`); λ replaces it with `E_SOLVER_DISJUNCTION_UNSUPPORTED` naming P3 as owner. | PASS |

## μ — reduced-gradient objective loop, optimality certificate (leaf)

| capability | binding | verdict |
|---|---|---|
| the optimality vocabulary is extensible | capability→producer, **wired on main**: `OptimalityStatus::{ProvenOptimal, BestFound{reason}, FeasibilityOnly}`, `BestFoundReason::{IterationLimit, ConvergedWithinBudget, Unreported}` (`reify-ir/src/ranked.rs`), consumed by `W_SOLVER_OPTIMALITY_UNPROVEN`. `FirstOrderStationary` is a new variant on an existing enum. | PASS |
| the single objective fold to preserve | capability→producer, **wired on main**: `eval_objective_set`. P3's stated constraint is that CP-SAT's `solve_ranked` scores through the **same** fold; μ calls it and does not replace it. Three fold sites already carry `debug_assert!` backstops against a fourth. | PASS |
| the null-space basis μ steps in | **producer:task-η UPSTREAM** (intra-batch edge μ←η). The QR that yields `Z` is η's deliverable — recorded as an edge, not assumed. | PASS |
| `∇f` availability | **producer:task-ε UPSTREAM** (edge μ←ε). | PASS |
| **#5016 is not contradicted** | DAG-direction/scope check: #5016 (`done`) ratified *NM + α-clustering + deterministic multistart*, rejecting MINLP and seeded global solvers — a ruling about the **global** strategy. μ replaces the **local** solver beneath it; multistart, clustering and ranking are untouched, and NM survives as λ's non-smooth escape. Recorded so a dispatch-time architect does not read this as re-litigation. | PASS |
| **premise under D3 verification** | The "today reports `Unreported`/`IterationLimit`" claim about `examples/continuous_cost_min.ri` is source-derived; leaf `mu` was submitted to the D3 workflow for behavioural confirmation. | PASS (see §D3) |

## ν — capability-based routing (leaf)

| capability | binding | verdict |
|---|---|---|
| the name-based classifier exists | capability→producer, **wired on main**: `ConstraintClassifier::classify` → `DomainFlags::into_domain`, `is_geometry_qualified_name` (11 literal names). | PASS |
| **`std::geo::*` is NOT a live prefix** | **anti-fiction check, greppable ABSENCE**: the compiler builds every stdlib qualified name as `format!("std::{}", name)`; `std::geo::` appears outside `classifier.rs` only in test files. The PRD is written against the 11-name `matches!` arm, not the dead prefixes. | PASS |
| the trait is extensible and the fix is pre-specified | **absorbs #6523 — EXECUTED.** #6523 was **cancelled into this leaf at decompose** after its content was ported into ν's `details` (the exact trait signature, the `domain_spec` delegation target from #5467, and the `PRECONDITION on the Logical slot` doc block whose deletion is the completion marker). Cancelled rather than parked because a pending task is dispatchable and would collide with ν on `decompose.rs`; an exhaustive reverse-dependency scan over 6696 tasks confirmed zero dependents, so nothing needed re-pointing. ν's files widened to `registry.rs` + `cpsat.rs`. | PASS |
| the `let`-boundary asymmetry is real | **rejection-check, OBSERVED**: probe `p_a_mixed.ri` — a `let`-bound `distance` call puts only `ValueRef`s in the constraint tree, classifies `Dimensional`, and **solves**, while the inline call classifies `Geometric` and dies. Two semantically identical models, two answers. | PASS |

## ξ — retire the 3D SolveSpace expression path (leaf)

| capability | binding | verdict |
|---|---|---|
| the retiring surface is delimited | capability→producer, **wired on main**: delete `SolveSpaceSolver`, `GeometricPattern`, `recognize_pattern`, `try_distance_eq`, `try_angle_eq`, `try_line_pair_constraint`, `extract_point_ref`, `extract_line_ref`, `add_pattern_to_builder`. **Keep** `SystemBuilder`, `get_sketch_normal`, `add_sketch`, `emit_sketch_constraint`, `solve_sketch`, all of `slvs_sys.rs` and `sketch.rs`. | PASS |
| the sketch path survives untouched | capability→producer, **wired on main**: `solve_sketch` takes a typed `SketchSystem` and never touches `ResolutionProblem` or the registry. **Honest note:** it has no production consumer outside its own crate and tests — keeping it keeps a landed substrate awaiting `constrained-2d-sketch`'s language surface, not live user-facing behaviour. | PASS |
| **the `antiparallel` premise is UNDER VERIFICATION** | `.contains("parallel")` precedes the antiparallel case in `recognize_pattern` — source-certain. But relate-block relations route through `relate_solve.rs`, which **has** a correct antiparallel residual, so the bug may be reachable only via the registry auto-param path. Leaf `xi` was submitted to the D3 workflow precisely to establish which path a user can reach it from, and to report honestly if it is unreachable from `.ri` source today. | PASS pending §D3 |

## ο — the `GeometryQuery` arm (leaf)

| capability | binding | verdict |
|---|---|---|
| ~~arity-2 `min_clearance(a, b)`~~ | **`producer-absent` — RESOLVED, NOT WAIVED.** The overload is belt leaf **#5441** (`pending`), outside this batch and not upstream of ο. Only the arity-3 kinematic `min_clearance(snapshot, a, b)` (`KINEMATIC_QUERY_NAMES`) exists. Signal re-homed. | ~~FAIL~~ → resolved |
| `distance(a, b)` — the replacement | capability→producer, **wired on main**: registered in `GEOMETRY_QUERY_NAMES` with a `geometry_query_result_type` arm, and probe-confirmed to evaluate (`p_a_mixed.ri` → `gap = 0.009999999999999997 m`) on the **build** path. The pure value-eval path refuses with an explicit message, so ο's signal is stated against `reify build`, not `reify eval`. | PASS |
| belt overloads ride for free | DAG-direction, no edge: the arm dispatches on the existing **`reify_ir::GeometryQuery`** variant, not a builtin name, so #5441–#5443's overloads become drivable when they land. No orphan, no dependency, no duplicated work. | PASS |
| **name collision caught** | `reify_ir::GeometryQuery` is **already** a `pub enum` (the kernel-query vocabulary: `Volume`, `MinDistance`, `FaceAnalyticDatum`, …, classified by `QueryCapability`). The trait is therefore `GeometryDispatch`, parallel to `ComputeDispatch`, and it **takes** the existing enum rather than inventing a query-kind vocabulary — a strictly better binding than the original. | PASS |
| the dependency-inversion shape | capability→producer, **wired on main**: `ComputeDispatch` (trait in `reify-ir`, impl `OptimizedComputeDispatcher` in `reify-eval`, consumed by `reify-constraints`) — task #4880, `done`. `reify-constraints/Cargo.toml` depends on **no** kernel crate and on no `reify-geometry`, so the inversion is forced, not chosen. | PASS |
| the `Option` anti-pattern is avoided deliberately | `ComputeDispatch::dispatch -> Option<Value>` conflates unregistered / failed / cancelled and callers fall back silently. `GeometryDispatchOutcome` is a typed 3-variant enum instead. Recorded so the precedent is copied in shape but not in defect. | PASS |
| **signed distance** | **producer:task-#6251 UPSTREAM** — hard `add_dependency` edge. All five interference/clearance queries bottom out in one **non-negative** `BRepExtrema_DistShapeShape`, which carries no penetration-depth gradient, so a clearance residual cannot push apart from inside — the Jacobian is identically zero exactly where the solver needs it. Until #6251 lands, a zero-valued clearance residual emits a tier-2 refusal rather than converging on a zero gradient. DAG-direction verified: #6251 depends on no leaf here. | PASS |
| **posed operands** | **producer:task-#6583 UPSTREAM — hard `add_dependency` edge, ADDED BY THE D3 RUN.** Measured: cross-sub `distance` does not resolve on the build path (`operator undefined for these operand kinds: StructureInstance` inline; undef cell when `let`-bound; identity pose under `>= 5mm`, `>= 500mm` and no constraint alike). The arm queries POSED operands; #6583 is what makes an operand carry its sub's placement. `pending`, high, no deps — no inversion. | PASS (was `producer-absent` before the edge) |
| arity-2 `min_clearance` is **silently accepted**, not rejected | rejection-check, OBSERVED as ABSENT: `reify check` on an arity-2 `min_clearance(a,b)` fixture exits 0 with `INDETERMINATE`, never a rejection. Sharpens the re-homing above — the missing overload is not even diagnosed. | PASS |

## π / ρ / σ — docs-truth gate (leaves)

| capability | binding | verdict |
|---|---|---|
| the vocabulary is genuinely undocumented | **greppable ABSENCE, re-verified this session**: `grep -rioE '\b(tangent\|concentric\|antiparallel\|coincident\|flush\|fasten\|at auto\|relate)\b'` across **all 17** chunks in `crates/reify-mcp/src/tools/chunks/` returns **nothing**. All ten `RELATION_FN_NAMES` are absent. | PASS |
| the omission detector cannot catch it | capability→producer, greppable: `reify-audit`'s PDOCCOVER census reads registries only from `crates/reify-compiler/src/units.rs`; `RELATION_FN_NAMES` lives in `relation_signatures.rs`, structurally outside the census. Recorded so π is not assumed redundant with an automated gate. | PASS |
| the corpus gate exists to satisfy | capability→producer, **wired on main**: `examples_smoke.rs::best_practices_index_matches_corpus_directory` (bidirectional file↔INDEX-row invariant) and `best_practices_constraint_gate.rs` (#6215: every constraint Satisfied, or pinned Indeterminate with a documented reason). ρ's file and its INDEX row must land in ONE commit. | PASS |
| chunk-leaf collision avoided | DAG-direction / scope split: belt **ν #5446** (at-auto+relate idiom, clearance family, `wire`, `belt_path`), sketch **θ #5513**, **#5389** (`in-progress`, existing clearance-oracle content), **#5347**. π documents the **relation vocabulary and the drivable-query idiom** only. No edge; scope recorded. | PASS |
| the spec amendment is narrow | π carries `docs/reify-language-spec.md` §10.3's Cross-domain row (the orchestrator framing survives; only "the classifier partitions, it cannot span" is corrected) and `constrained-2d-sketch.md` §11's O3-scoped refinement. No spec rewrite. | PASS |

## τ — PRD close (leaf)

| capability | binding | verdict |
|---|---|---|
| terminal vocabulary is closed and known | capability→producer: exactly three values (`SHIPPED` / `SUPERSEDED` / `WITHDRAWN`), matched case-insensitively on the first token after the `Status` label. Freeze-header shape per `v0_6/data-carrying-enums.md` and `kernel-seam-contracts.md`. | PASS |
| cancelled-sibling disposition | recorded: a `cancelled` sibling counts as satisfied for τ's dependency edge; if the scheduler treats it as unmet, the decompose steward removes the edge by hand and applies the stamp in a docs-only commit rather than leaving τ permanently blocked. | PASS |
| dependency shape | τ depends on **every** other leaf via real `add_dependency` edges. | PASS |

---

## §D3 — substrate-verification workflow

`scripts/prd-decompose-verify.mjs` was run over the four leaves whose premises were **not** already probe-confirmed in the authoring session — `theta`, `xi`, `omicron`, `mu`. The remaining leaves' current-state premises are bound above to either a committed fixture run this session (β, κ), a greppable production wiring, or a greppable absence. Its verdict is recorded in the decompose hand-back; any leaf it returns `FAIL`/`UNPROVABLE` for has its signal re-homed before the batch is released.
