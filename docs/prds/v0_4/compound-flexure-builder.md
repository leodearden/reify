# PRD: Generic Compound-Flexure Builder (compose_flexures([...]))

Status: **BOOKMARK — deferred, unauthored slot.** No decomposable tasks exist until
activation criteria fire. Filed 2026-05-17 from task 3823 per
[[preferences_bookmark_task_pattern]].

## Goal

Provide a generic `compose_flexures([...])` builder that lets users compose bespoke
compound flexure topologies beyond the three baked-in v0.3 primitives (parallelogram,
double-parallelogram, cartwheel). Target use-cases include butterfly flexures,
Roberts-approximation variants, multi-stage parallelogram chains, and other topologies
that appear in real printer-build or instrument-design dogfood but cannot be expressed
as a single primitive call. The builder would accept an ordered sequence of primitive or
sub-assembly flexures and produce a composed compliant mechanism with unified PRB
(Pseudo-Rigid-Body) parameters and stiffness characterisation.

## Why bookmarked (not yet authored)

Designing a generic composition API without a concrete topology to anchor it produces
speculative abstraction. The three v0.3 primitives cover the documented use-cases in
the current printer-design dogfood. Compound topologies have not yet appeared in any
`.ri` file or sibling PRD mechanism requirement. Authoring this PRD before demand
materialises would invent an API surface with no validation signal and risk premature
commitment to a composition model that real usage might immediately refute.

v0.3 ships the three primitives and the parallelogram + cartwheel composition surface
(cross-PRD seam, `compliant-joints-flexures.md` §9). Wait for the first concrete
compound-topology demand to inform the design.

## Activation criteria

This slot becomes activatable when **both** of the following hold:

1. **Demand signal** — at least one of:
   - A concrete `.ri` dogfood file names a compound flexure topology that is not
     expressible as one of the three v0.3 primitives (parallelogram,
     double-parallelogram, cartwheel); OR
   - A sibling PRD's §2.2 mechanism table names such a topology as a required
     mechanism type.

2. **Scale fork** — evaluated at activation time:
   - **≥ 2 distinct compound topologies queued** → build the generic
     `compose_flexures([...])` builder (author and decompose this PRD).
   - **Exactly 1 compound topology needed** → add a fourth primitive instead (a
     smaller-scope task, not this PRD). This PRD remains deferred.

The ≥ 2-vs-1 decision is made at activation time, not now. Do not resolve it
preemptively.

## What activation will produce (non-binding sketch)

*This section is a rough orientation only. The real design and task decomposition
happen when the PRD is activated.*

- `compose_flexures([f1, f2, ...])` Reify DSL function accepting an ordered list of
  primitive or sub-assembly flexures; returns a composed compliant mechanism.
- Unified PRB parameter extraction for the composed topology.
- Stiffness characterisation (serial/parallel composition rules, or fallback to direct
  FEA for topologies where closed-form composition does not apply).
- Consumes the v0.3 parallelogram + cartwheel composition surface seam named in
  `compliant-joints-flexures.md` §9 (cross-PRD seam row: "produces … parallelogram +
  cartwheel composition surface; generic builder consumes").

## Out of scope until activated

- This stub **produces no decomposable tasks**. Orchestrators and PRD-decomposers
  **must not** attempt to queue, decompose, or implement anything from this file while
  its status reads `BOOKMARK — deferred`.
- API surface, parameter names, composition rules, and task DAG are all undesigned and
  must not be inferred from the non-binding sketch above.

## Relationship / provenance

| Item | Reference |
|---|---|
| Source-side bookmark breadcrumb | `docs/prds/v0_3/compliant-joints-flexures.md` §2.4 (lines 175–182) |
| Cross-PRD "produces" row | `docs/prds/v0_3/compliant-joints-flexures.md` §9 (line 527) |
| Phase task that filed this bookmark | `docs/prds/v0_3/compliant-joints-flexures.md` Phase 6 ν (lines 661–664) |
| Tracker task | Task 3823 |
| Bookmark convention | [[preferences_bookmark_task_pattern]] |
