# PRD: A-Posteriori Error Estimation and Auto-Refinement

Status: design resolved + decomposed (2026-05-05) — deferred, candidate v0.4. Long-term refinement of the v0.3 progressive solve. Filed 2026-05-02 from FEA PRD spillover.

## Goal

Compute a-posteriori error indicators on FEA results so that users can see whether their answer is well-converged or merely coarse-mesh noise. Drive automatic mesh refinement to a user-requested accuracy budget. Closes the "is my answer good enough?" question that every FEA practitioner faces.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships P1/P2 tetrahedral FEA with a "progressive solve" framework (task #15) that runs coarse-then-refine on demand. But the demand signal is currently external — the user (or auto-resolve loop near a constraint boundary) explicitly requests refinement. There is no automatic feedback from the solver's own confidence.

Real users need:
- **Confidence signal** — per-element or per-region indicator of how trustworthy the local result is.
- **Auto-refinement** — when accuracy is inadequate, the mesh adapts itself (locally refine high-error regions, leave low-error regions alone).
- **Budget control** — refine until either target accuracy is reached or a budget cap (max DOFs, max wallclock) is hit.

The standard answer is **a-posteriori error estimation**: a numerical method that uses the FEA result itself to estimate its own error. Zienkiewicz-Zhu superconvergent patch recovery is the workhorse — compare element stress to a smoothed nodal stress field; the difference is an error indicator. Other techniques (residual-based, hierarchical, dual / goal-oriented) trade off complexity against accuracy of the indicator.

This closes the FEA workflow loop: solve → estimate error → refine → re-solve, automatically, to the user's accuracy budget.

## Why deferred to v0.4+

- Needs **v0.3 FEA + progressive solve** as foundation (tasks #15, #16, #17 of the FEA PRD). Building error estimation before the solver exists is backwards.
- **Refinement implementation** requires non-uniform mesh refinement support in the mesher — an extension to the v0.3 Gmsh integration that doesn't currently exist.
- **UX design** for budget knobs, refinement convergence, and confidence signaling needs concrete user pain to inform — speculative design here is wasteful.
- Substantial numerical work; benefits from waiting until v0.3 generates user feedback that prioritizes which error-estimator to invest in first.

## Sketch of approach

Three pieces:

1. **Error indicator** — start with Zienkiewicz-Zhu (Z-Z) patch recovery: nodal stress is interpolated from element stresses (volume-weighted average over element patches around each node); per-element error indicator = norm of (element-stress − interpolated-nodal-stress). Cheap to compute, well-validated on linear elasticity, doesn't require a separate solve.
2. **Refinement strategy** — h-refinement (subdivide elements with high error indicator into smaller elements). Mark elements above an adaptive threshold (e.g. top 20% by indicator), pass marking to mesher for local subdivision. Remesh with the refinement directive; re-solve.
3. **Loop control** — outer loop: solve → estimate → refine → re-solve, until either (a) global error indicator drops below user threshold, (b) max iterations hit, (c) DOF count cap hit. Per-iteration progress visible to user (and to auto-resolve loop).

User-visible API: extend `ElasticOptions` with `target_accuracy : Number = 0.05` (5% relative error default), `max_refinement_iterations : Integer = 5`, `max_dofs : Integer = 5_000_000`. When set, solver runs the refinement loop instead of single-shot.

## Pre-conditions for activating

- v0.3 FEA kernel + progressive solve shipped (`structural-analysis-fea.md` tasks #15, #16).
- Mesher supports non-uniform local refinement (extension to task #17, may need new Gmsh API surface).
- Result interpolation handles changing meshes across iterations cleanly (extension of task #13).
- Auto-resolve loop ready to consume "current accuracy estimate" signal (otherwise the value of the indicator is unclear).

## Resolved design decisions (2026-05-05)

- **Error indicator: Zienkiewicz-Zhu (Z-Z) superconvergent patch recovery** for v0.4. Per-element indicator = norm of (element stress − patch-averaged nodal stress). Cheap (no auxiliary solve), well-validated on linear elasticity, ~1k LOC of pure Rust over the existing per-element stress field from kernel task #2920. Rejected residual-based (needs aux solve, more code, rigor matters mainly for adversarial cases Reify users don't have). Rejected DWR — best for goal-oriented but needs Reify-language syntax for "quantity of interest" + an adjoint solve of comparable cost; substantial language workstream, real v0.5+. (See DWR future-proofing below.)
- **Refinement strategy: h-refinement only.** P3+ tetrahedral shape functions are not in v0.3 (only P1/P2), so adaptive p needs new element kinds first. hp is research-grade with stability/conditioning headaches. h-only is what commercial tools mostly run.
- **Refinement marking: Dörfler marking with θ = 0.5.** Mark elements in descending order of indicator until cumulative indicator reaches θ × global. Provable convergence guarantees on broad problem classes. Rejected fixed-top-N% — works in practice but lacks theory.
- **Budget knobs**: three user-facing, "any of these stops it" semantics — `target_accuracy: Number = 0.05` (relative energy-norm error), `max_refinement_iterations: Integer = 5`, `max_dofs: Integer = 5_000_000`. Plus implicit **stall termination** (if global indicator drops <10% iter-over-iter, stop) so pathological cases don't iterate to budget cap unproductively. Budget-capped-without-target uses *warning + downgraded confidence flag*, never hard error — interactive design is iterative.
- **Error norm**: relative *energy-norm* error globally (the engineering-standard quantity); per-element *stress-norm* indicator for visualisation. Different metrics for different purposes; both named explicitly in the result API as `global_relative_energy_error: Number` and `per_element_indicator: Field<...>`.
- **Confidence signaling — substrate now, UX later.** Substrate locked: norms above; visualisable per-element field on the surface mesh; `convergence_status: enum { Converged(target), NotConverged { reason: BudgetReason } }` on `ElasticResult` so downstream code (auto-resolve, CI gates) can check before trusting numeric values. UX (default presentation, percentage vs confidence band, on-by-default vs toggle, phrasing for non-FEA users) deferred to user research after v0.4 ships and produces real designer feedback.
- **Auto-resolve composition: refinement gated on auto-resolve's "near constraint boundary" classification.** Far-from-boundary probes pass `target_accuracy = 0.10` (coarse is enough); near-boundary probes pass `target_accuracy = 0.01` (the inadequate-resolution case where refinement earns its cost). Per-probe accuracy contract is the plumbing. Refinement does NOT fire on every probe — only on the auto-resolve loop's "accept" / final-answer step or on explicit user request.
- **Refinement vs mesh morphing — lazy refinement at decision time.** Refinement breaks connectivity, morph preserves it; they're fundamentally incompatible at the topology level. Composition resolved by *timing*: solve at coarse mesh (with morph) for fast UI response → on user pause OR auto-resolve convergence, run the refinement loop, invalidate morph cache, full remesh. Subsequent parameter changes return to morphed coarse mesh until next pause. Cost paid once per "I'm settled on these values" moment, not every parameter probe. Document the cost honestly — there is no clever fix.
- **DWR future-proofing**: `ElasticOptions.target_quantity_of_interest: Option<QoIDescriptor>` parameter, *accepted but ignored* in v0.4. Cheap to design now (one optional field on the existing options struct), becomes the DWR driver in v0.5+ without breaking existing usage. The `QoIDescriptor` type in v0.4 is a stub enum with no variants — first variants added when DWR lands.
- **GUI integration**: per-element error indicator field rides on the existing v0.3 FEA GUI rendering pipeline (task #2962 contour rendering) as another selectable scalar channel in the FEA-mode dropdown. Probe popup (task #2964) extends with a local error-indicator readout at the probed point. One small task, no new architecture.
- **Mesher choice**: continue with **Gmsh size-field-driven local refinement** for v0.4 (extends task #2925 with per-vertex size hints). Switch to **MMG3D** as v0.4.x bookmark if Gmsh's regenerate-from-scratch cost dominates the refinement loop. Decision criterion: switch if a typical refinement loop spends >30% of wallclock in remeshing.

## Out of scope for this PRD

- Goal-oriented (DWR) error estimation as a first-class indicator — accepted-but-ignored hook ships in v0.4 (see Resolved decisions); first DWR variants in v0.5+ when language adds quantity-of-interest syntax.
- Time-dependent / nonlinear error estimation — depends on transient / plasticity PRDs that don't exist yet.
- Spatial coarsening (un-refinement of over-resolved regions) — useful for very long parameter sweeps; defer until refinement ships.
- Multi-physics coupled error estimation — depends on multi-physics PRDs that don't exist yet.

## Task decomposition

Seven active tasks plus one deferred bookmark. Tasks gate on v0.3 FEA kernel completion (`structural-analysis-fea.md`).

1. **Z-Z error indicator** — pure-Rust Zienkiewicz-Zhu patch recovery on top of the per-element stress field from kernel task #2920. Returns `Field<Element, ScalarPressure>` (per-element indicator) plus a global `Number` (relative energy-norm error). No auxiliary solve. ~1k LOC + unit tests against textbook patch tests.
2. **Refinement loop control + budget enforcement** — outer loop (solve → estimate → mark → refine → re-solve), three budget knobs + stall detection, Dörfler marking with θ=0.5. Owns the `ConvergenceStatus` enum and termination-reason bookkeeping.
3. **ElasticResult API extensions** — extend `ElasticResult` with `error_indicator: Option<Field<Element, ScalarPressure>>`, `global_relative_energy_error: Option<Number>`, `convergence_status: ConvergenceStatus`. Extend `ElasticOptions` with `target_accuracy`, `max_refinement_iterations`, `max_dofs`, `target_quantity_of_interest: Option<QoIDescriptor>` (DWR future-proofing hook, no variants yet).
4. **Gmsh size-field-driven local refinement** — extend the v0.3 Gmsh integration (task #2925) to accept per-vertex size hints; marked elements get reduced size; remesh with new size field. Mesh content hash changes → cache key changes → cache invalidates correctly.
5. **Lazy-refinement timing + auto-resolve composition** — refinement fires on auto-resolve's "accept" step or explicit user request, never on every parameter probe. Per-probe `target_accuracy` contract (0.10 far-from-boundary, 0.01 near-boundary). Morph cache invalidates on refinement; document the perf trade.
6. **GUI: error indicator scalar channel** — add `errorIndicator` as a selectable channel in the FEA-mode toolbar (rides on tasks #2961 + #2962). Probe popup (#2964) extends with local indicator readout.
7. **Validation suite (analytical convergence study)** — convergence study against L-shaped domain (known re-entrant-corner singularity), plate-with-hole (known stress concentration), cantilever (smooth case as control). Demonstrate Z-Z indicator drops at expected rate; demonstrate auto-refinement total cost ≤ manual mesh-refinement baseline.

**Deferred bookmark**: MMG3D mesher swap (v0.4.x). Switch criterion: if a typical refinement loop spends >30% of wallclock in remeshing under Gmsh, swap to MMG3D's local-remesh path.

## Implementation status (2026-09)

A dated note on where the eval-side wiring actually stands, because the
mesher-facing half of this PRD landed in two stages and the interim stage was
deliberately, honestly, *not* adaptive.

- **Loop control (decomposition #2) lives in `reify-solver-elastic`**
  (`adaptive.rs`): the `AdaptiveProblem` trait, `run_adaptive_refinement`,
  Dörfler marking at θ = 0.5, budget + stall termination. It is mesher-agnostic
  by construction — the caller supplies `refine`.

- **Task #4902 threaded that loop into `reify-eval`'s `solve_elastic_static`
  with a MESH-FREE UNIFORM refine step**, labelled as non-adaptive rather than
  dressed up as adaptive (ratified esc-4902-83, option D). Two constraints
  forced it: production `reify-eval` is gmsh-build-free (#4743 makes
  `reify-kernel-gmsh` a dev-dep), and the synthetic `nx×1×nz` box that path
  builds has no closed surface `Mesh` for `refine_marked_elements` to remesh
  from. The marked set was computed correctly and then discarded; each
  iteration simply doubled the grid on every axis.

- **Task #4909 closes that interim.** Eval-side refinement is now genuinely
  mark-driven on the *realized* `body : Solid` path opened by #4870: the
  Dörfler set reaches `refine_marked_elements` (decomposition #4's
  size-field-driven local refinement), which remeshes under a per-element size
  field. Refinement is localized to the marked region rather than applied
  uniformly.

### Where the surface comes from, and why

`refine_marked_elements` needs the closed surface its volume mesh was meshed
from. On the realized path no such `Mesh` reaches the consumer: a
`RealizationReadHandle` structurally carries exactly ONE `RealizedContent`
variant, and for `solver::elastic_static` that variant is the `VolumeMesh`.
Adding a second (surface) realization demand was rejected — it would mean
changing `reify-ir` / `reify-compute-contract` / the engine's realization
plumbing, a blast radius far outside a wiring task, on a content-hashed and
bincode-cached type.

Instead the boundary is **reconstructed from the realized tet mesh's own free
faces** (`reify_solver_elastic::volume_refine::boundary_surface_mesh`). This is
not merely the cheaper route, it is the tighter one: the size field is
transferred onto the surface by a nearest-vertex scan, so a boundary whose
vertices are a bit-equal SUBSET of the volume mesh's makes every lookup a
distance-0 identity — strictly better than an independently tessellated surface
of the same solid, which would share no vertices with the tet mesh at all.

### Fallback

The gmsh-realized lane is selected at RUNTIME on
`reify_solver_elastic::GMSH_AVAILABLE`, never on a `cfg` (a cfg emitted by the
gmsh crate's build script does not propagate to dependents, so a cfg gate would
read false on every host). It also requires a realized mesh, an extractable
boundary, and the absence of selector-resolved BCs — those are node INDEX sets
resolved against the pre-refinement mesh, and a remesh preserves no index.
Any unmet precondition, or a `RefineError` raised at runtime, falls back to
#4902's uniform lane with a Warning naming the reason. An `adaptive: true`
request never degrades to a failed solve because the gmsh lane could not run.

### Still open

Decomposition **#7 (validation suite / analytical convergence study)** is NOT
closed by #4909, which deliberately asserts no convergence *rate*: its
localization assertions are strict inequalities on element counts and on
before/after sizes at a fixed physical position. Adaptive-vs-uniform log-log
rate validation remains task #3002's remit.

Also note: `displacement` / `stress` / `max_von_mises` and the other primary
result fields still reflect the INITIAL seed mesh; only `convergence_status`,
`global_relative_energy_error` and `error_indicator` reflect the refinement
loop. That is #4902's ratified v1 contract, surfaced to callers as an Info
diagnostic, and re-deriving the primary fields from the refined mesh would move
the §7a resample grid that #4910 depends on.

> This PRD edit rides task #4909's branch through the merge queue rather than
> committing direct-to-main: #4909 is a mixed code+docs change, so the
> docs-only fast path does not apply to it.

## Test plan

Validation rests on convergence-study cases with known analytical answers or established benchmark singularities:

- **L-shaped domain** (re-entrant corner produces a r^(2/3) stress singularity) — adaptive refinement should drive the global indicator down at the optimal asymptotic rate; uniform refinement does worse. The classic adaptive-FEM sanity check.
- **Plate-with-hole under uniaxial tension** — stress concentration factor ≈ 3 at the hole; indicator should fire on hole-perimeter elements, refinement should resolve the concentration.
- **Cantilever under tip load** — smooth solution; serves as control where adaptive refinement should converge similarly to uniform refinement.
- **Auto-resolve composition test** — drive a parameter `thickness = auto` against `max_von_mises ≤ 200 MPa`; verify refinement fires only on near-boundary probes (instrumented via per-probe `target_accuracy` reads) and that the converged thickness matches a hand-tuned reference within 5%.
- **Morph composition test** — slide a parameter through 10 values without refinement firing (morph cache hits); on user-pause-equivalent (explicit request), verify refinement fires once, morph cache invalidates, subsequent slides re-establish the morph cache from the refined mesh.

CI gate: indicator-drop-rate must hit ≥ 70% of the asymptotic optimal for L-shaped (specific bound TBD during impl); convergence_status correctly reports termination reason for each test case.

## Relationship to other PRDs and tasks

- **Extends `structural-analysis-fea.md` task #15 (progressive solve)** — replaces the manual-trigger refinement with an automatic feedback loop driven by the error indicator.
- **Extends `structural-analysis-fea.md` task #17 (mesher)** — needs non-uniform local refinement support.
- **Composes with `mesh-morphing.md`** — refinement triggers remesh; morph is reset.
- **Benefits `multi-load-case-fea.md`** — accuracy guarantees apply per-case, with the refinement budget shared or per-case.
- **Generalises naturally to `structural-analysis-shells.md`** — shells benefit from error estimation too; same indicator with element-kind dispatch.
