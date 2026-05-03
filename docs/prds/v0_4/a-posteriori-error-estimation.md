# PRD: A-Posteriori Error Estimation and Auto-Refinement

Status: stub — deferred, candidate v0.4+. Long-term refinement of the v0.3 progressive solve. Filed 2026-05-02 from FEA PRD spillover.

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

## Open design questions

- **Error indicator pick** — Z-Z is the standard cheap option; residual-based is more rigorous but requires solving auxiliary problems; goal-oriented (DWR) is best when the user cares about a specific output (e.g. max von Mises, deflection at a probe point) rather than global error. Lean: Z-Z first, others if v0.4 user feedback demands.
- **Refinement strategy** — h-refinement (more elements) is simple; p-refinement (raise element order) is sometimes better; hp-refinement combines both. Lean: h-only for v0.4; revisit if needed.
- **Budget knobs** — what's the right user-facing knob? Target accuracy? Wallclock budget? Max DOFs? Some combination? Probably all three with an "any of these stops it" semantics.
- **Confidence signaling** — what does "the answer has 3% error" mean to a non-FEA-expert designer? UX research needed.
- **Interaction with auto-resolve** — auto-resolve already uses progressive solve to short-circuit infeasible regions; how does that compose with auto-refinement? Probably: auto-resolve gets a coarse-then-refine ladder per parameter value; refinement only kicks in near constraint boundaries.
- **Refinement vs. mesh morphing** — if mesh morphing is in play (`mesh-morphing.md`), how does refinement compose? Probably: morph keeps the same connectivity; refinement breaks that and triggers a remesh. Need clean fallback semantics.

## Out of scope for this PRD

- Goal-oriented (DWR) error estimation — useful but heavier; v0.5+ if user demand emerges.
- Time-dependent / nonlinear error estimation — depends on transient / plasticity PRDs that don't exist yet.
- Spatial coarsening (un-refinement of over-resolved regions) — useful for very long parameter sweeps; defer until refinement ships.
- Multi-physics coupled error estimation — depends on multi-physics PRDs that don't exist yet.
- Auto-refinement during auto-resolve outer loop — composition design depends on both maturing first.

## Relationship to other PRDs and tasks

- **Extends `structural-analysis-fea.md` task #15 (progressive solve)** — replaces the manual-trigger refinement with an automatic feedback loop driven by the error indicator.
- **Extends `structural-analysis-fea.md` task #17 (mesher)** — needs non-uniform local refinement support.
- **Composes with `mesh-morphing.md`** — refinement triggers remesh; morph is reset.
- **Benefits `multi-load-case-fea.md`** — accuracy guarantees apply per-case, with the refinement budget shared or per-case.
- **Generalises naturally to `structural-analysis-shells.md`** — shells benefit from error estimation too; same indicator with element-kind dispatch.
