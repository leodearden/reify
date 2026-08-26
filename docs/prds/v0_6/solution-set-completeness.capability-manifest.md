# Capability manifest — `docs/prds/v0_6/solution-set-completeness.md`

Built at decompose, 2026-08-26. Leaf task IDs: α #6706 · β #6707 · γ #6708 · δ #6709 · ε #6710 · ζ #6711 · η #6712 · θ #6713 · ι #6715 · λ #6716 · κ #6718 · μ #6719. Mechanizes G3 + G6 per leaf: every capability each leaf's signal asserts, bound to evidence. Any FAIL binding blocks the batch.

**Code anchors** verified against main `2128c3692c` (2026-08-26). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Probe provenance.** Behavioural bindings marked *observed* were run at the anchor commit with `target/debug/reify eval` against the committed fixtures under `tests/prd-gate/fixtures/`; grammar bindings with `tree-sitter parse --quiet` from `tree-sitter-reify/`. Exit codes and verbatim diagnostics are recorded in PRD §0.2.

---

## Standing caveat — cross-PRD edges (two WIRED, two still deferred)

**P1 (`solver-driver-parity.md`) landed as `108d1d9226` and decomposed into 15 leaves (#6689–#6704) during this session's decompose phase**, so its two edges are wired as real `add_dependency` edges. **P2 (`geometry-algebra-solver-unification.md`, committed `2ea72cc5e8`) has not decomposed** — its leaves carry no task IDs and its own capability manifest is still untracked — so its two edges have no task ID to depend on:

| This leaf | Needs | Why |
|---|---|---|
| ζ (#6711) | **P1 γ = #6691** — deterministic axis order (I4 / T1) | **WIRED.** ζ is the first multi-`auto` leaf; C6 in full is false without it |
| η (#6712) | **P1 κ = #6699** — warm and cold choose the same solve entry point | **WIRED.** A basin change is otherwise indistinguishable from an entry-point artefact |
| θ (#6713) | **P2 ι** — relate-solve through the unified problem | **DEFERRED** (P2 unstamped). P2 ξ deletes `SolveSpaceSolver`; fixing it here would be work P2 removes |
| γ (#6708) | **P2 ε** — forward-mode AD (soft) | **DEFERRED** (P2 unstamped). Preferred Jacobian source; finite differences are the sound fallback |

The two deferred ones are filed as a named prose prerequisite on each leaf. **Wiring them as real `add_dependency` edges once P2 decomposes is an explicit decompose-steward obligation.** A leaf whose cross-PRD prerequisite is still unwired must not be dispatched on the strength of its intra-batch edges alone. This is recorded rather than silently accepted because an unwired hard edge is exactly the shape that produces a fake-done leaf.

---

## α (#6706) — `Completeness` / `SolutionSet` carrier in `reify-ir`

| Capability | Evidence | Verdict |
|---|---|---|
| ranked carrier exists and is wired | capability→producer — `RankedSolveResult`, `RankedCandidate`, `OptimalityStatus`, `BestFoundReason` in `crates/reify-ir/src/ranked.rs`, wired on the production path via `DimensionalSolver::solve_ranked_impl` and both `engine_eval.rs` consumption seams (not test-only). Grep-verified. | **PASS** |
| additive-only is the required shape | anti-inversion — F-result invariant I1 freezes `SolveResult` and `ConstraintSolver::solve()`. The production `engine_eval` / `engine_edit` / `concurrent` matches destructure `Solved { values, unique }` **without** `..`, so adding a field would not compile; a sibling carrier is the only back-compatible shape. α is additive. | **PASS** |
| completeness precedent to generalise | capability→producer — `SolveAllResult { solutions, complete }` in `crates/reify-constraints/src/cpsat.rs`, landed by PRD 2 β **#5468** (done, `2ba8b4dd38`). Its doc already states the conjunction doctrine this PRD promotes. | **PASS** |

## β (#6707) — honesty floor (adopts #5388)

| Capability | Evidence | Verdict |
|---|---|---|
| the uniqueness path exists where β edits it | capability→producer — `verify_uniqueness` and `finalise_uniqueness` in `crates/reify-constraints/src/solver.rs`; reached from `solve_with_meta` (whole result) and `solve_ranked_impl` (winner only). Grep-verified. | **PASS** |
| the message β rewrites actually fires | G6 branch 4 (rejection observed, not assumed) — `tests/prd-gate/fixtures/ssc_two_roots_strict.ri`, **observed**: exit 1, `error: strict auto parameter resolution is not uniquely determined — consider using auto(free) for exploration`, `note: … is undef (because: solve failed: infeasible)`. β modifies an emission that demonstrably fires; it does not have to create a rejection mechanism. | **PASS** |
| the code-less sites exist and are informationless | capability→producer — five byte-identical `Diagnostic::warning` sites (`push_merged_cluster_nonunique_warnings` and the cold arm in `engine_eval.rs`, the `eval_cached` arm, two arms in `engine_edit.rs`), none carrying a `DiagnosticCode`. **Observed** emitting identical text on `ssc_two_roots_free.ri` (two roots) and `ssc_single_root_free.ri` (one root). | **PASS** |
| typed undef causes exist to extend | capability→producer — `pub enum UndefCause` in `crates/reify-ir/src/value.rs` (`Unbound`/`AwaitingSolve`/`SolveFailed`/`OpContractFailed`/`UserUndef`), the INV-SF-1 precedent. | **PASS** |
| β does not invert a dependency on #5711 | anti-inversion — **#5711** (in-progress) edits the same `verify_uniqueness` arm. It is a sibling, **not downstream** of β; the PRD requires coordination, not an edge that would point backwards. | **PASS** |
| β does not change solved-cell determinacy | signal premise (scope rail a, PRD §8) — β changes what is *claimed*, never what is *computed*; a strict auto whose set is `Partial` still resolves and stays `Determined`. | **PASS** (manual) |

## γ (#6708) — box branch-and-bound enumerator

| Capability | Evidence | Verdict |
|---|---|---|
| HC4 propagator, sub-box capable | capability→producer — `producer:task-6655` (pending, **no deps, dispatchable**), whose charter delivers `crates/reify-constraints/src/{interval.rs,hc4.rs}` and explicitly requires the propagator run on a sub-box for this PRD. **Upstream** of γ via a real `add_dependency` edge. | **PASS** |
| no interval substrate is being assumed to exist | anti-orphan — grep-verified **absent** on main: no interval arithmetic, contractor, box consistency, HC4 or branch-and-bound anywhere in `crates/`. `DerivedInterval` / `derive_param_intervals` is a one-pass syntactic bound miner over four linear-in-one-auto shapes, not interval arithmetic. This is why #6655 is a hard prerequisite rather than a background assumption. | **PASS** |
| a Jacobian is available for the uniqueness test | capability→producer — preferred: **P2 leaf ε** (forward-mode dual numbers over `CompiledExpr`), **UNSTAMPED** so unwireable (see standing caveat). Unconditional fallback: local finite differences. Soundness is unaffected either way — a failed uniqueness test simply keeps splitting (C8) — so the leaf does not block on P2. | **PASS** via fallback |
| angle domains do not double-count | correctness precondition (G7 INV-AD-2) — `default_bounds_for` gives `ANGLE → (−τ, τ)`, a double cover of the circle; §3.3 requires canonicalisation to one period before `Exhaustive` may be claimed, else the completeness machinery manufactures a doubled count. | **PASS** (manual) |

## δ (#6709) — verdict policy

| Capability | Evidence | Verdict |
|---|---|---|
| toleranced equality verdicts | capability→producer — `producer:task-6653` (pending, high). Without it a correctly-solved root is re-checked at exact `f64` and reported `violated` (probe-observed on five fixtures), so δ's fixtures could not be green end-to-end. **Upstream** edge. | **PASS** |
| fixtures parse | grammar-fixture — `tests/prd-gate/fixtures/{ssc_two_roots_free,ssc_two_roots_strict,ssc_single_root_free}.ri`, each `tree-sitter parse --quiet` exit 0, **0 ERROR nodes**, 2026-08-26. No novel syntax (PRD §2.3). | **PASS** |
| the asserted baseline is the measured baseline | G6 branch 3 — every "today" claim δ's signal contrasts against is **observed**, with exit codes and verbatim text in PRD §0.2, not inferred from code reading. | **PASS** |
| the silence assertion is falsifiable | G6 branch 4 (negative assertion) — BT4 asserts the non-uniqueness warning **stops** firing on `ssc_single_root_free.ri`. The warning is **observed firing there today**, so the assertion has an observable before-state and cannot pass vacuously. | **PASS** |
| δ's signal does not depend on an unstamped edge | anti-inversion — δ's fixtures are single-`auto` by design, so C6's multi-`auto` ordering claim (P1 γ, unstamped) is not on δ's critical path; it is asserted at ζ instead. | **PASS** |

## ε (#6710) — refutation by subdivision

| Capability | Evidence | Verdict |
|---|---|---|
| the empty-box proof arm exists upstream | capability→producer — `producer:task-6655` delivers the whole-box empty ⇒ typed proven-infeasible arm; ε extends it to the subdivision case. **Upstream** edge. | **PASS** |
| ε's fixture is not already #6655's | **producer-extent guard (deliberate)** — `tests/prd-gate/fixtures/ssc_refuted_pair.ri` (`L == 10mm` ∧ `L == 20mm`) parses clean and **observed** today as exit 1 / `error: constraints could not be satisfied (max absolute residual: 5.00e-3)`. It is refuted by HC4 **at the root box**, so #6655 alone turns it green. It is committed as C3 baseline evidence, **not** as ε's signal. ε's own fixture — a root box HC4 does not empty, refuted only by subdivision — cannot be authored or verified before #6655's propagator exists, so **constructing it and demonstrating root-box HC4 leaves it undecided is part of ε's acceptance**. | **PASS** (manual; obligation stated on the leaf) |

## ζ (#6711) — stop discarding the alternatives

| Capability | Evidence | Verdict |
|---|---|---|
| the alternatives are produced today | capability→producer — `DimensionalSolver::solve_ranked_impl` produces `K = 2·(dim+1)` ranked candidates; both `engine_eval.rs` consumption seams `swap_remove(0)` and discard `candidates[1..]`. `RankedSolveResult` / `RankedCandidate` appear in **no crate outside** `reify-ir` / `reify-constraints` / `reify-eval`. Grep-verified. The producer exists and the consumer is absent — ζ **is** the consumer. | **PASS** |
| the rendering surface exists | capability→producer — `reify explain` (`cmd_explain`) + `ObjectiveProvenance`, landed by `constraint-solver-completion.md` θ **#4015** / ι **#4017** (both done), wired on the production CLI path. It carries no optimality, uniqueness or completeness token today. | **PASS** |
| a dedup key is genuinely missing | signal premise — `solve_ranked_impl` documents that non-winning candidates are **not deduplicated**, so for a single-basin objective most of K are near-identical convergences of one optimum. C5's box identity is the missing key. | **PASS** |
| deterministic ordering | **DAG-direction, UNWIREABLE** — P1 leaf γ is upstream but unstamped (standing caveat). Recorded as `external_deps` + steward obligation. | **PASS**, edge deferred |

## η (#6712) — basin stability across warm re-solves

| Capability | Evidence | Verdict |
|---|---|---|
| warm paths exist to instrument | capability→producer — `edit_param` (`crates/reify-eval/src/engine_edit.rs`) seeds `current_values` from the prior snapshot **including previously solved autos**; reached from the GUI `set_parameter` and `crates/reify-cli/src/mcp_context.rs`. `Engine::eval_cached` is the second warm path (LSP). | **PASS** |
| the cold-only hole is real | capability→producer — `W_SOLVER_OPTIMALITY_UNPROVEN` and `detect_underdetermined` have production call sites only in `Engine::eval`, not `eval_cached`; `dispatch_merged_cluster_solve_cached` documents the consequence in-tree. | **PASS** |
| entry-point parity | **DAG-direction, UNWIREABLE** — P1 leaf κ upstream, unstamped (standing caveat). η owns basin identity; P1 owns parity. | **PASS**, edge deferred |

## θ (#6713) — C1 conformance

| Capability | Evidence | Verdict |
|---|---|---|
| the over-claiming sites exist | capability→producer — `SolveSpaceSolver::solve` hardcodes `unique: true` on every libslvs `Ok` arm and on the empty-autos arm (and it **is** in `SolverRegistry::production()`'s Geometric slot); `relate_solve::solve_frame` sets `unique = fully_pinned && !unknown.free`, where `fully_pinned` is local Jacobian rank. Grep-verified. | **PASS** |
| θ does not fix code P2 deletes | **anti-inversion, resolved by re-scoping** — P2 leaf **ξ** deletes `SolveSpaceSolver` outright and leaf **ι** routes relate-solve through the unified problem. θ was re-scoped at authoring from "fix two drivers" to "C1 conformance over whatever writes `unique` after P2 ι", and sequenced after it. Unstamped (standing caveat). | **PASS**, edge deferred |
| the in-progress collision is sequenced | anti-inversion — **#6659** is in-progress with a live claimant editing `registry.rs` / `solvespace.rs`; θ is sequenced after it rather than racing it. | **PASS** |

## ι (#6715) — the composition law

| Capability | Evidence | Verdict |
|---|---|---|
| a conjunction already exists to replace | capability→producer — `SolverRegistry::solve_inner` conjoins per-component `unique` (`all_unique` / `other_unique`) and forces `unique = false` on intermediate lexicographic ranks. The §3.5 meet replaces that conjunction in place. | **PASS** |
| ι does not depend on PRD 2 ζ | anti-inversion — ι implements the meet over **components**, which exist today (multiple continuous components per model). PRD 2 ζ **#5472** is a *consumer* of the law, not a producer of ι's capability, so ι is not blocked on a pending PRD-2 leaf. | **PASS** |
| the registry collision is sequenced | anti-inversion — after **#6659** (in-progress, live claimant, same file). | **PASS** |

## λ (#6716) — envelope calibration

| Capability | Evidence | Verdict |
|---|---|---|
| the budget introduces no wall-clock bound | numeric floor / C7 — the enumeration budget is a **node count**, never seconds. `tests/infra/test_no_new_wallclock_upper_bounds.sh` therefore needs no new registration, and determinism survives a loaded machine. | **PASS** |
| the cap is measured, not guessed | G6 branch 1 — the cap value is **deliberately not asserted at authoring**; λ *is* the measurement, which is branch 1's sanctioned resolution for an otherwise-guessed threshold. The only stated neighbouring datum is the measured Nelder-Mead simplex knee (~10–15 vars; `WHOLE_MODEL_CLUSTER_DIM_CAP = 12`), used as an ordering hint, not as the bound. Enumeration and refutation get **separate** caps because refutation needs no uniqueness test and prunes harder. | **PASS** |

## κ (#6718) — docs-truth

| Capability | Evidence | Verdict |
|---|---|---|
| the doc chunk gap is real | capability→producer — `crates/reify-mcp/src/tools/chunks/constraints.md` contains **zero** uniqueness / multiple-solution content (grep-verified for `unique`, `multiple solution`, `root`, `basin`: no hits). κ is its producer. | **PASS** |
| the exemplar carries a false claim κ corrects | G6 branch 3 — `examples/best_practices/discrete_choice.ri` states "Real branch-and-bound over discrete variables is what CP-SAT will bring". **False for its own file:** `side` is a continuous `Real` auto with no finite domain, and CP-SAT requires one via `build_variable_domain`, so the CP-SAT path can never reach it. Rung 2 serves this exemplar, not rung 1. | **PASS** |
| the index edit is gated | capability→producer — `best_practices_index_matches_corpus_directory` build-gates `examples/best_practices/INDEX.md` against the corpus directory; the row edit is same-diff with the exemplar edit. | **PASS** |

## μ (#6719) — PRD close

| Capability | Evidence | Verdict |
|---|---|---|
| the terminal vocabulary and header shape are specified | capability→producer — the closed three-value terminal vocabulary (`SHIPPED` / `SUPERSEDED` / `WITHDRAWN`) and the three-part freeze header are specified in the PRD overlay; in-corpus exemplars `docs/prds/v0_6/data-carrying-enums.md` and `docs/prds/kernel-seam-contracts.md`. | **PASS** |
| the close leaf stays dispatchable | anti-inversion — μ depends on every sibling by real `add_dependency` edges; a `cancelled` sibling counts as satisfied for the edge, per the overlay's cancelled-dependency disposition. | **PASS** |

---

**No FAIL bindings. The batch is not blocked.** Two bindings remain `PASS, edge deferred` — both are P2 edges named in the standing caveat, and both are steward obligations rather than substrate gaps. The two P1 edges were wired during this session once P1 landed and decomposed.
