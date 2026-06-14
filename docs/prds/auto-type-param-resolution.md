# PRD: `auto` Type-Parameter Resolution (`Bearing<auto: Seal>`)

Status: Superseded by docs/prds/v0_3/auto-type-param-resolution-completion.md (v0.3 completion contract). Applied after residuals α/β/γ/δ landed.

## §0 — Superseded by docs/prds/v0_3/auto-type-param-resolution-completion.md

v0.1 shipped the per-parameter BFS `auto:` resolver: candidate enumeration,
feasibility filtering via the trait-conformance predicate, lexicographic
tiebreak, and a cap of 10 candidates per parameter. The combinatorial
backtracking it deferred to v0.2, plus the substitution pass, constraint-aware
selection, value population, and BFS-fallback soundness work that make `auto:`
user-observable, are completed by the v0.3 completion contract (whose §0 and
§15 name this supersession). This PRD is retired — consult the v0.3 completion
contract (`docs/prds/v0_3/auto-type-param-resolution-completion.md`) for
current behaviour, and the v0.2 PRD
(`docs/prds/v0_2/auto-resolution-backtracking.md`) as the search-algorithm
design source-of-truth.

## Goal

Resolve `auto` in **type-parameter** position (e.g. `sub bearing : Bearing<auto: Seal> { bore_diameter = 25mm }`) to a concrete type at elaboration time, before value-parameter resolution. The chosen algorithm is a per-parameter BFS over in-scope candidates that satisfy the trait/kind bound, capped at 10 candidates per parameter, with deterministic lexicographic tiebreak. No cross-parameter backtracking in v0.1 — parameters resolve in declared order.

## Background

- Spec §3.9 (lines 500-512): `auto` for type parameters means "I want this to be some specific type; system, figure out which one."
  - Step 1: enumerate candidates — all types satisfying the trait/kind bound that are in scope.
  - Step 2: filter by feasibility — instantiate with each candidate and check whether the resulting constraints are satisfiable.
  - Step 3: select — exactly-one feasible → strict `auto`; multiple feasible → strict is an error, `auto(free)` selects deterministically (lexicographic by FQN).
  - Resolution happens at elaboration time, before value-param resolution.
- Architecture §6.2 (line 619): "Auto type resolution" is one of six topology-change sources. `Bearing<auto: Seal>` resolved to `Bearing<ORingSeal>` triggers re-elaboration of the containing scope and its descendants because the resulting type changes the schema.
- Architecture §2.1 SchemaNode (line 132 area) and §6.3 elaboration-evaluation cycle: re-elaboration with the resolved concrete type produces a new topology snapshot; downstream nodes either reuse cached results (path-based identity) or recompute.
- Architectural decision Q5 (already approved):
  - **Per-param BFS**, not joint search across all auto type-params.
  - **Cap pool at 10 candidates per param.** If more than 10 satisfy the bound, error with "candidate pool exceeds cap" pointing at the bound and listing the first 10 alphabetically.
  - **Lexicographic tiebreak by fully qualified name.**
  - **No cross-param backtracking.** Earlier params resolve first; later params see the earlier choices as fixed.
  - Combinatorial backtracking deferred to v0.2.
- Existing infra: #66 (trait conformance) provides the predicate "does type T satisfy trait `Seal`?" — this is what filters candidates by the bound. The feasibility filter (step 2) further reuses the value-`auto` solver mechanism: instantiate, evaluate constraints, accept if no constraint flips to `false`.

## Scope

### Phase A: candidate enumeration

- Walk the in-scope name table at the use site and collect every concrete type whose trait/kind bound is satisfiable per #66.
- Trait-bound (`T: Seal`): every concrete type implementing `Seal` (directly or via trait composition).
- Kind-bound (`N: Nat`, `Q: Dimension`): types tagged with the kind. Cap doesn't matter for `Nat` since `Nat` candidates are typically supplied by the user as literals — `auto: Nat` is uncommon; keep the algorithm uniform but warn loudly.
- Composite (`T: TraitA + TraitB`): intersection.
- Cap at 10. If more than 10 in scope, emit error `E_AUTO_TYPE_PARAM_POOL_OVERFLOW` listing first 10 (alphabetical) and pointing at the bound; ask user to disambiguate by importing only the candidates they want or by writing the type explicitly.

### Phase B: per-candidate feasibility filter

- For each enumerated candidate (in alphabetical order, up to 10):
  1. Instantiate the parameterized definition with the candidate substituted for the auto type-param.
  2. Run the value-`auto` machinery's constraint feasibility check on the resulting scope's constraints (re-using the same primitives ResolutionNode uses internally — see arch §2.5).
  3. Accept the candidate if no constraint resolves to `false` after partial propagation.
- Feasibility is **monotonic in known constraints**: if a constraint is `undef` (depends on as-yet-`undef` parameters), treat it as feasible (consistent with arch §2.5 — undef does not falsify).

### Phase C: selection

- 0 feasible → `E_AUTO_TYPE_PARAM_NO_CANDIDATE` listing each rejected candidate with the constraint that ruled it out.
- 1 feasible → use it.
- ≥ 2 feasible:
  - **Strict `auto`** (the default): error `E_AUTO_TYPE_PARAM_AMBIGUOUS` listing the ≥ 2 feasible candidates with the lexicographically first highlighted as the suggested explicit choice.
  - **`auto(free)`**: pick lexicographic-first by FQN; emit warning `W_AUTO_TYPE_PARAM_NON_UNIQUE`.

### Phase D: topology trigger

- Once resolved, the SchemaNode re-elaborates with the concrete type substituted (architecture §6.2 row 5, §6.3 cycle).
- Downstream nodes invalidate as needed; warm-state pool keyed by node-type + path-based identity (arch §6.4) lets cache survive when the same concrete type was previously chosen.
- Multiple auto type-params in one definition resolve **in declared order**; each later param's candidate enumeration sees the earlier param already substituted (no backtracking across params in v0.1).

### Diagnostics

- All errors and warnings include:
  - The bound (`T: Seal`) and the use-site span.
  - The full candidate list considered (alphabetical, capped at 10).
  - For each rejected candidate, the constraint(s) that ruled it out (when known).
- For ambiguity, an explicit-substitution suggestion: `Bearing<ORingSeal>` instead of `Bearing<auto: Seal>`.

## Out of scope

- Cross-param backtracking / joint search (deferred to v0.2).
- Searching across module boundaries beyond the current import set. Auto-type-param search uses **only types currently in scope**.
- Generic specialization heuristics (e.g., prefer derived-most type).
- `auto` over higher-kinded types or type lambdas.
- Performance optimization beyond the cap-of-10 cutoff.

## Acceptance criteria

1. **Single-candidate happy path:** Define `trait Seal` with one impl `ORingSeal`. `Bearing<auto: Seal>` resolves to `Bearing<ORingSeal>`; the resulting structure elaborates and value-param resolution proceeds.
2. **Two-candidate ambiguity (strict):** Two `Seal` impls. `Bearing<auto: Seal>` errors with `E_AUTO_TYPE_PARAM_AMBIGUOUS` listing both candidates.
3. **Two-candidate `auto(free)`:** Same setup; `Bearing<auto(free): Seal>` picks the lexicographically first candidate and warns `W_AUTO_TYPE_PARAM_NON_UNIQUE`.
4. **Pool overflow:** ≥ 11 `Seal` impls in scope. `Bearing<auto: Seal>` errors with `E_AUTO_TYPE_PARAM_POOL_OVERFLOW` listing first 10 alphabetically.
5. **No-candidate / infeasibility:** Two `Seal` impls but a constraint excludes both. Error `E_AUTO_TYPE_PARAM_NO_CANDIDATE` lists each rejection reason.
6. **Multi-auto-param declared order:** `Coupling<auto: A, auto: B>` — `A` resolves first; `B`'s candidate pool is computed against the resolved `A`. Test that swapping declared order changes which candidates appear feasible (documents declared-order semantics).
7. **Topology trigger:** Resolution flips a SchemaNode's topology fingerprint; downstream nodes re-elaborate. Warm-state pool reuses prior cached node results when the same candidate is re-selected after a parameter edit + revert.
8. **Kind-bound `auto: Nat`:** Either errors loudly with a "kind-bound auto unsupported in v0.1" diagnostic, or is supported; pick the simpler path and document.
9. **Composite bound `auto: TraitA + TraitB`:** Candidate pool is the intersection. Tested with at least one candidate satisfying both, one satisfying only one (excluded).
10. **Diagnostics surface in LSP** with the full candidate list, rejection reasons, and explicit-substitution suggestion.
11. **Determinism:** Same source produces same resolution choice across runs and across machines (lexicographic-by-FQN tiebreak is canonical).
12. **Performance:** Per-param resolution cost is bounded — at most 10 candidates × per-candidate feasibility check. Document the cost and ensure it does not regress baseline elaboration time on the v0.1 example corpus.

## Task breakdown

1. **Candidate enumeration pass.** Walk the in-scope type table at the use site, collect concrete types satisfying the bound (trait / kind / composite). Cap at 10 with `E_AUTO_TYPE_PARAM_POOL_OVERFLOW`. Reuse #66 conformance predicate. Tests: trait, kind, composite bounds; cap overflow.
2. **Per-candidate feasibility filter.** For each candidate, instantiate the definition with the substitution, run the value-auto solver's feasibility primitives on the resulting scope, treat undef constraints as feasible. Tests: 0/1/multi feasible.
3. **Selection logic.** Strict-vs-free arms; lexicographic tiebreak by FQN; emit the right error/warning per case. Tests: each error/warning code with golden diagnostic strings.
4. **Multi-param declared-order resolution.** Loop over auto type-params in declared order; thread the resolved substitution into subsequent candidate enumerations. Tests: reordered declaration changes outcome.
5. **Topology trigger.** Wire the resolution into SchemaNode.compute() so the resolved concrete type substitutes into the topology template and re-elaboration proceeds (arch §6.2-6.3). Tests: topology fingerprint change, warm-state pool reuse on revert.
6. **Diagnostic + LSP integration.** All four codes (POOL_OVERFLOW, NO_CANDIDATE, AMBIGUOUS, NON_UNIQUE) surface via the standard diagnostics path with full candidate listings and explicit-substitution suggestion. Tests: LSP diagnostic snapshot.
7. **Determinism / smoke tests.** Add an example file exercising `Bearing<auto: Seal>` end-to-end. Run twice, assert identical resolved snapshot hash. Run on the v0.1 example corpus to confirm no regression.
8. **Documentation.** Update language-spec / stdlib reference cross-refs and add a "How `auto` type-param resolution works" doc with the algorithm, the cap-of-10, and the deferred-to-v0.2 cross-param backtracking note.

## Open questions deferred to implementation

- Where exactly the feasibility filter draws the line between "constraint depends on undef and we don't know" vs "constraint is decisively false." Reuse architecture §2.5 / value-auto primitives; document any deviation.
- Whether kind-bound auto (`auto: Nat`) should ship in v0.1 at all. If usage is unclear, error with a "not supported in v0.1" diagnostic and revisit in v0.2.
- Whether the v0.1 cap should be 10 or smaller (e.g., 5). 10 is the architectural-decision default — adjust only with explicit signoff and update this PRD.

## Dependencies

- #66 (trait conformance): provides the satisfiability predicate for trait bounds.
- Existing value-`auto` solver primitives in ResolutionNode.compute() (architecture §2.5): feasibility filter reuses these.
