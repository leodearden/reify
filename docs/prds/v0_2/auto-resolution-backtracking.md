# PRD: Combinatorial `auto` Type-Parameter Resolution Backtracking

Status: deferred to v0.2 per 2026-04-26 decision.
Design resolved 2026-04-28 — see "Resolved design decisions" below.
Completion: the decomposition of `docs/prds/v0_3/auto-type-param-resolution-completion.md`
has landed — residuals α/β/γ/δ (tasks 4431/4433/4434/4435, covering substitution (α),
constraint-aware selection (β), BFS-fallback soundness (γ), and value population (δ)) are merged.
v0.2 `auto`-resolution backtracking is DONE. This v0.2 PRD remains the design
source-of-truth for the search algorithm; the v0.3 completion contract's §4–§8 supply
the missing apply/evaluate/soundness/value-population contracts and formally supersede
the v0.1 parent (`docs/prds/auto-type-param-resolution.md`).

## Goal

Lift the v0.1 restriction on `auto` type-parameter resolution. v0.1 resolves each `auto` type parameter independently via per-parameter BFS over candidate types with no backtracking across parameters. v0.2 adds bounded-depth combinatorial search over the cross-product of candidates, with backtracking when the per-parameter winner produces a globally infeasible instantiation. This is `docs/reify-language-spec.md` §3.9 + `docs/reify-implementation-architecture.md` §6.2 territory.

## Background

Spec §3.9 defines `auto` for type parameters: "I want this to be some specific type; system, figure out which one." Resolution enumerates candidate types satisfying the trait/kind bound, instantiates with each, checks constraint feasibility, and selects the unique feasible candidate (strict) or one of several deterministically (`auto(free)`).

When a single declaration has multiple `auto` type parameters, the candidates form a cross-product. v0.1 ships a per-parameter BFS that resolves each parameter independently in declaration order — feasibility is checked one parameter at a time. This is fast and produces deterministic results, but it's incomplete: when parameter A's locally-feasible choice rules out every parameter B, the algorithm fails even though some other choice for A would have left a feasible B.

The architecture explicitly accepts this limitation for v0.1 and queues the full combinatorial search for v0.2.

## Why deferred

- Per-parameter BFS handles the overwhelming majority of real cases: declarations with one `auto` type parameter, or with multiple `auto`s whose feasibility regions don't overlap.
- The pathological cases (multiple coupled `auto`s) tend to be design smells anyway — users can usually rewrite to break the coupling.
- Full backtracking adds nontrivial implementation complexity (state for the search tree, pruning heuristics, depth bounds, diagnostic generation across the search) and is most usefully designed once the per-parameter scheme has been used in anger and the actual failure cases are documented.
- Failure mode is graceful: when v0.1 BFS fails, the diagnostic enumerates candidates and rejection reasons, so the user can see what went wrong and intervene.

## Sketch of approach

The v0.2 algorithm extends v0.1's per-parameter loop to a depth-first search over the cross-product of candidate sets, with constraint feasibility evaluated incrementally as each parameter is bound. When binding parameter K to candidate C produces an infeasible partial instantiation, the search backtracks and tries the next candidate for K. If K's candidates are exhausted, it backtracks to K−1.

Bounded depth: the search is cut off at a configurable depth (likely 4–6 parameters by default) to prevent combinatorial blowup. Above the bound, the algorithm falls back to per-parameter BFS with a diagnostic noting the bound was hit.

Pruning: the constraint feasibility check produces a "rejected because" reason that often indicates which already-bound parameter is responsible. The search uses this to backjump rather than backtrack one level — a classical CSP optimization.

Determinism: candidate enumeration is lexicographic by fully qualified name (already specified in v0.1 §3.9 for `auto(free)`). The search tree is therefore deterministic and the resulting selection is reproducible across runs.

Diagnostics: when the search fails completely, the diagnostic reports the parameters considered, the cross-product size, the bound (if hit), and the smallest infeasibility witness — the partial assignment that ruled out the most candidates. When multiple feasible solutions exist under strict `auto`, the diagnostic enumerates them.

## Pre-conditions for activating

- v0.1 alpha has shipped with per-parameter BFS.
- The test suite for v0.2 includes documented v0.1 BFS-failure cases (not a gate on real-user-pain telemetry — algorithm shape is correct; documented cases drive test design).

## Resolved design decisions (2026-04-28)

**Default depth bound: 6 parameters.** Above 6, fall back to v0.1 per-parameter BFS with a diagnostic noting the bound was hit. Reasoning: at depth 4 with a typical 4–8 candidates per slot, cross-product is 256–4096; at depth 6, 4096–262k. Sub-second worst case at typical constraint-check cost. Beyond 6, constraint cost dominates and loud failure beats opaque search. Configurable via project metadata for unusual cases.

**Cross-product hard cap: 100k assignments.** Independent guard from depth bound — depth caps parameter count, cap caps search space *given* the bound. When the cross-product would exceed 100k, refuse with a diagnostic naming the parameters and asking the user to constrain further. Falls back to v0.1 BFS like the depth-bound case.

**`auto(free)` under cross-product search: report-all, pick-one.** When multiple cross-product assignments are feasible under `auto(free)`, the diagnostic enumerates all feasible assignments up to a display cap of 16 (further entries elided with a count), and the runtime picks the lexicographically-first one by fully-qualified name. Strictly better than v0.1 per-parameter `auto(free)`, which loses cross-product information.

**Backjumping reuses existing "rejected because" channel.** v0.1 BFS already produces these reasons for diagnostics. v0.2 search just consumes them — when binding parameter K's candidate produces a rejection blamed on parameter J<K, search backjumps to J rather than backtracking K−1. No new infrastructure on the constraint-checker side.

**Constraint-feasibility incremental binding deferred.** PRD originally listed this as a precondition. Re-classified: implement v0.2 search with full re-check at each binding; measure; optimize only if it bites. Tracker for the optimization filed separately as a v0.2.x bookmark task.

**Diagnostic format on search failure.** Reports: parameters considered (in declaration order), candidate counts per parameter, cross-product size, depth/cap-hit flag, and the *smallest infeasibility witness* — the partial assignment that ruled out the most candidates downstream. This last is the most user-actionable piece; everything else is context.

**Determinism.** Candidate enumeration lexicographic by FQN (already specified for v0.1 `auto(free)` per spec §3.9). Search tree therefore deterministic; resulting selection reproducible across runs and machines.

## Out of scope for this PRD

- General SMT-style constraint propagation (search is over discrete type choices; constraint feasibility for a fixed assignment is the existing checker).
- Probabilistic / heuristic selection when many feasible solutions exist (still lexicographic by FQN).
- Cross-declaration `auto` coordination (each declaration's `auto`s are resolved independently from each other declaration's).
- Value-parameter `auto` resolution scheme — that's the scope-level coupled solver in arch §11.4–§11.5, separate concern.
- Constraint-feasibility incremental-binding optimization (separate v0.2.x bookmark task; revisit when telemetry shows quadratic-re-check cost is the bottleneck).
