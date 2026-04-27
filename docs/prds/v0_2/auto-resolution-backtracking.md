# PRD: Combinatorial `auto` Type-Parameter Resolution Backtracking

Status: deferred to v0.2 per 2026-04-26 decision.

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

- v0.1 alpha has shipped with per-parameter BFS and at least a handful of cases have been documented where users hit the limitation.
- The constraint feasibility checker (already used by v0.1 BFS) supports incremental binding well enough to be called inside a search loop without quadratic re-checking.
- Someone has written down what a reasonable default depth bound is, based on real usage.

## Out of scope for this PRD

- General SMT-style constraint propagation (the search is over discrete type choices; constraint feasibility for a fixed assignment is the existing checker).
- Probabilistic / heuristic selection when many feasible solutions exist (still lexicographic by FQN).
- Cross-declaration `auto` coordination (each declaration's `auto`s are resolved independently from each other declaration's).
- Value-parameter `auto` resolution scheme — that's the scope-level coupled solver in arch §11.4–§11.5, separate concern.
