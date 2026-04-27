# PRD: Per-Purpose Representation Tolerance Contract

Status: deferred to v0.2 per 2026-04-26 decision.

## Goal

Implement the bidirectional representation tolerance contract from `docs/reify-implementation-architecture.md` §10.4 and §14.4: tolerance is a property of purposes, RealizationNodes are keyed by `(entity, repr_kind, tolerance)`, and the runtime allocates an error budget across conversion chains. Imported geometry carries a tolerance promise that the runtime cannot verify but consumers may rely on.

## Background

v0.1 ships a single global tolerance setting for all geometric realizations. This is enough to make geometry visible in the GUI and to export STEP files that downstream tools accept, but it conflates two real distinctions:

1. **Purpose-driven tolerance** — manufacturing wants tight bounds (e.g. 1µm); interactive visualization wants loose-and-fast (e.g. 0.1mm). Forcing one global value either makes interactive editing slow or makes manufacturing exports inaccurate.
2. **Conversion chain accumulation** — B-rep → mesh → SDF → voxel accumulates error at every step. With a single global tolerance, the runtime has no principled way to allocate sub-budgets.

The architecture envisions tolerance as living primarily at the purpose level (§10.4), with entity-level escape hatches. Multiple simultaneous purposes (§14.4) explicitly resolve via separate RealizationNodes keyed by tolerance — a tighter realization may opportunistically satisfy a looser request.

## Why deferred

- A single global tolerance is workable for v0.1 because the kernel is OCCT-only (no conversion chain) and the GUI can fix a reasonable visualization tolerance for everyone.
- Per-purpose tolerance is tightly coupled to multi-kernel dispatch (`multi-kernel.md`) — without conversion chains, there's no budget to allocate.
- Tolerance budget allocation is open question §16 #1 in the architecture; the heuristics aren't designed yet.
- Imported-geometry tolerance interacts with the `imported` field source (`imported-field-source.md`), which is also v0.2.

## Sketch of approach

Tolerance enters the language at the purpose level — purposes already declare `RepresentationWithin(subject, tolerance)` constraints (see arch §14.1). The runtime extracts these into a tolerance scope: every entity reachable from the purpose's subject inherits the tolerance bound unless a tighter entity-level override is in scope. Output occurrences (`STEPOutput`, etc., §14.5) carry their own bounds that combine with the active purpose's bounds.

RealizationNodes get a third key dimension: cache lookup becomes `(entity_id, repr_kind, tolerance_class)`. Tolerance class is a small ordered set (e.g. coarse / standard / tight / micron) rather than continuous floats — this keeps the cache space manageable while letting tighter realizations be reused for looser demands. A "tighter satisfies looser" rule is a cache-hit optimization, not a correctness mechanism: the runtime always *may* recompute at the requested tolerance.

The conversion chain budget is a runtime heuristic. When a request crosses kernel boundaries (B-rep → mesh → SDF), the orchestrator divides the bound across stages roughly proportional to each stage's expected error — exact allocation is open question §16 #1 and will likely start as a fixed fraction (e.g. half budget per conversion) and evolve based on telemetry.

Imported geometry gets a designer-declared tolerance via `Input` occurrence parameters. The runtime treats it as an assertion (used for budget allocation downstream) and a promise (cannot be verified for arbitrary STEP/STL input). When a downstream demand is tighter than the import promise, the runtime emits a diagnostic rather than silently producing an over-confident realization.

## Pre-conditions for activating

- v0.1 alpha has shipped with single global tolerance and at least one user has hit a real "interactive vs. manufacturing" tension.
- Multi-kernel dispatch (`multi-kernel.md`) is in flight or the design is locked in — conversion chains are the primary motivator for budget allocation.
- Purpose declarations are stable; the v0.1 `purpose def` syntax should already accept `RepresentationWithin` so v0.2 just activates dormant infrastructure rather than introducing new syntax.

## Out of scope for this PRD

- Design tolerance / GD&T (orthogonal — see arch §10.4 last paragraph).
- Tolerance stack-up analysis (RSS, Monte Carlo) — separate v0.2 item, arch §16 deferred #7.
- Runtime tolerance auto-tuning based on solver convergence (post-v0.2).
