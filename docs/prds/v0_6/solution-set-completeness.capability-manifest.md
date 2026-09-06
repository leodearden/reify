# Capability manifest — `docs/prds/v0_6/solution-set-completeness.md`

Built at decompose, 2026-08-26. Leaf task IDs: α #6706 · β #6707 · γ #6708 · δ #6709 · ε #6710 · ζ #6711 · η #6712 · θ #6713 · ι #6715 · λ #6716 · κ #6718 · μ #6719. Mechanizes G3 + G6 per leaf: every capability each leaf's signal asserts, bound to evidence. Any FAIL binding blocks the batch. **E3 A/B UPDATE (2026-09-03, task #7221):** ten of these twelve leaves — β #6707, γ #6708, δ #6709, ε #6710, ζ #6711, η #6712, θ #6713, ι #6715, λ #6716, κ #6718 — were retired to `deferred` on 2026-08-28 and replaced by the coarse tasks #6900 (γ+ε+λ), #6901 (β+δ), #6902 (ζ+η+κ) and #6903 (θ+ι) under `docs/prds/v0_6/e3-decomposition-granularity-ab.md` §5; they remain §11 **rollback targets**, so their bindings stay live rather than being deleted. α #6706 and μ #6719 are reused in place as singletons and are still the live producers. The four coarse tasks carry **hand-authored** `metadata.delivered_checks` that are *not* derived from this manifest and have no sidecar of their own, so this file remains the record for the twelve standard leaves only.

**Code anchors** verified against main `2128c3692c` (2026-08-26). Main moves fast — cite-by-symbol; re-locate lines at implementation time.

**Probe provenance.** Behavioural bindings marked *observed* were run at the anchor commit with `target/debug/reify eval` against the committed fixtures under `tests/prd-gate/fixtures/`; grammar bindings with `tree-sitter parse --quiet` from `tree-sitter-reify/`. Exit codes and verbatim diagnostics are recorded in PRD §0.2.

Machine-readable twin: `solution-set-completeness.capability-manifest.yaml`. Its `delivered_check` bindings are the **post-delivery** state (pattern-anchored ERE, never `file:line`), so they are delivery assertions rather than evidence of the current defect — most of them **fail today by design**, and the `binding`/Evidence column records the current-state measurement in prose instead. An α row whose evidence describes the *existing* ranked carrier while its pattern greps for the *not-yet-existing* `enum Completeness` is therefore the sanctioned shape, not a mismatch to "fix". `kind: manual` entries are recorded but excluded from the dispatch gate. All 11 mechanical checks were re-executed with the gate's own invocation (`git grep -E -e <pattern> <ref> -- <paths>`) at main `d6909c3497` on 2026-09-03: 11 FAILED, 0 DELIVERED, 0 ERRORED — i.e. every one resolves in the asserted direction and none is vacuous.

---

## Standing caveat — cross-PRD edges (RESOLVED: three WIRED, one deliberately soft)

**Both siblings have now decomposed** — P1 as `108d1d9226` (15 leaves #6689–#6704), P2 as `961228d217` (19 leaves #6668–#6687). Every edge below is resolved. Three are real `add_dependency` edges; the fourth is deliberately left unwired because it is soft, and wiring it would block this batch's enumerator on a whole sibling PRD for an accuracy improvement a sound fallback already covers:

| This leaf | Needs | Why |
|---|---|---|
| ζ (#6711) | **P1 γ = #6691** — deterministic axis order (I4 / T1) | **WIRED.** ζ is the first multi-`auto` leaf; C6 in full is false without it |
| η (#6712) | **P1 κ = #6699** — warm and cold choose the same solve entry point | **WIRED.** A basin change is otherwise indistinguishable from an entry-point artefact |
| θ (#6713) | **P2 ι = #6677** — relate-solve through the unified problem | **WIRED.** P2 ξ #6682 deletes `SolveSpaceSolver`; fixing it here would be work P2 removes |
| γ (#6708) | **P2 ε = #6672** — forward-mode AD | **SOFT, deliberately UNWIRED.** Preferred Jacobian source; finite differences are an unconditional sound fallback (C8), so a hard edge would only delay γ |

**No steward obligation remains outstanding on this axis.** The two corrections this PRD's §7.1 recorded as owed to P2 were executed by the P3 session (prd-reify-135745) in `9e5662ad51`.

**Three further P2 seams carry no edge but must not be lost** (full statements in PRD §7.1):

1. **`reify-ir/ranked.rs` co-tenancy.** P2 μ **#6680** adds `BestFoundReason::FirstOrderStationary` **replacing `Unreported`**, in the same file α **#6706** adds the `completeness` field to. α must not hard-code `Unreported` in its default trait lift; whichever lands second reconciles.
2. **`FirstOrderStationary` does NOT satisfy C2.** `‖Zᵀ∇f‖` is a **local** first-order stationarity certificate. C2 requires `Exhaustive`. Promoting a stationary point to `ProvenOptimal` once P2 lands is a plausible-looking and wrong refactor — a stationary point in one basin says nothing about other basins, which is precisely the false-completeness claim this PRD exists to prevent, arriving from the optimality side.
3. **Declined components need a verdict.** P2 ν **#6681** makes capability-based routing able to *decline* a component. ι **#6715**'s meet must treat a declined component as `Partial` carrying P2 δ **#6671**'s typed refusal reason — never `NotAttempted`, which θ retires.

---

## α (#6706) — `Completeness` / `SolutionSet` carrier in `reify-ir`

| Capability | Evidence | Verdict |
|---|---|---|
| ranked.rs co-tenancy with P2 μ #6680 | seam — μ adds `BestFoundReason::FirstOrderStationary` **replacing `Unreported`** in the same file. α must key its default trait lift off "whatever the trait default reports", not the named `Unreported` variant; whichever leaf lands second reconciles. Separately, `FirstOrderStationary` is a **local** certificate and must NOT be read as satisfying **C2** (`ProvenOptimal` requires `Exhaustive`). | **PASS** (manual) |
| ranked carrier exists and is wired | capability→producer — `RankedSolveResult`, `RankedCandidate`, `OptimalityStatus`, `BestFoundReason` in `crates/reify-ir/src/ranked.rs`, wired on the production path via `DimensionalSolver::solve_ranked_impl` and both `engine_eval.rs` consumption seams (not test-only). Grep-verified.<br>`grep:enum[[:space:]]+Completeness` → `present` in `crates/reify-ir/src` | **PASS** |
| additive-only is the required shape | anti-inversion — F-result invariant I1 freezes `SolveResult` and `ConstraintSolver::solve()`. The production `engine_eval` / `engine_edit` / `concurrent` matches destructure `Solved { values, unique }` **without** `..`, so adding a field would not compile; a sibling carrier is the only back-compatible shape. α is additive. | **PASS** |
| completeness precedent to generalise | capability→producer — `SolveAllResult { solutions, complete }` in `crates/reify-constraints/src/cpsat.rs`, landed by PRD 2 β **#5468** (done, `2ba8b4dd38`). Its doc already states the conjunction doctrine this PRD promotes. | **PASS** |

## β (#6707) — honesty floor (adopts #5388)

| Capability | Evidence | Verdict |
|---|---|---|
| the uniqueness path exists where β edits it | capability→producer — `verify_uniqueness` and `finalise_uniqueness` in `crates/reify-constraints/src/solver.rs`; reached from `solve_with_meta` (whole result) and `solve_ranked_impl` (winner only). Grep-verified. | **PASS** |
| the message β rewrites actually fires | G6 branch 4 (rejection observed, not assumed) — `tests/prd-gate/fixtures/ssc_two_roots_strict.ri`, **observed**: exit 1, `error: strict auto parameter resolution is not uniquely determined — consider using auto(free) for exploration`, `note: … is undef (because: solve failed: infeasible)`. β modifies an emission that demonstrably fires; it does not have to create a rejection mechanism.<br>`grep:consider using auto\(free\)` → `absent` in `crates/reify-constraints/src` (pattern narrowed 2026-09-03, task 7221: the full sentence `consider using auto(free) for exploration` is unmatchable by a line-oriented `git grep` — `solver.rs:3496-3498` splits the literal across three Rust string-continuation lines and writes the em-dash as `\u{2014}` — so the un-narrowed form read `DELIVERED` vacuously from authoring; the narrowed form matches `solver.rs:3497`, rc=0) | **PASS** |
| the code-less sites exist and are informationless | capability→producer — five byte-identical `Diagnostic::warning` sites (`push_merged_cluster_nonunique_warnings` and the cold arm in `engine_eval.rs`, the `eval_cached` arm, two arms in `engine_edit.rs`), none carrying a `DiagnosticCode`. **Observed** emitting identical text on `ssc_two_roots_free.ri` (two roots) and `ssc_single_root_free.ri` (one root). | **PASS** |
| typed undef causes exist to extend | capability→producer — `pub enum UndefCause` in `crates/reify-ir/src/value.rs` (`Unbound`/`AwaitingSolve`/`SolveFailed`/`OpContractFailed`/`UserUndef`), the INV-SF-1 precedent. | **PASS** |
| β does not invert a dependency on #5711 | anti-inversion — **#5711** (in-progress) edits the same `verify_uniqueness` arm. It is a sibling, **not downstream** of β; the PRD requires coordination, not an edge that would point backwards. | **PASS** |
| β does not change solved-cell determinacy | signal premise (scope rail a, PRD §8) — β changes what is *claimed*, never what is *computed*; a strict auto whose set is `Partial` still resolves and stays `Determined`. | **PASS** (manual) |

## γ (#6708) — box branch-and-bound enumerator

| Capability | Evidence | Verdict |
|---|---|---|
| HC4 propagator, sub-box capable | capability→producer — `producer:task-6655` (pending, **no deps, dispatchable**), whose charter delivers `crates/reify-constraints/src/{interval.rs,hc4.rs}` and explicitly requires the propagator run on a sub-box for this PRD. **Upstream** of γ via a real `add_dependency` edge.<br>`grep:fn[[:space:]]+.*subdivide\|enum[[:space:]]+BoxVerdict\|fn[[:space:]]+enumerate_solutions` → `present` in `crates/reify-constraints/src` | **PASS** |
| no interval substrate is being assumed to exist | anti-orphan — grep-verified **absent** on main: no interval arithmetic, contractor, box consistency, HC4 or branch-and-bound anywhere in `crates/`. `DerivedInterval` / `derive_param_intervals` is a one-pass syntactic bound miner over four linear-in-one-auto shapes, not interval arithmetic. This is why #6655 is a hard prerequisite rather than a background assumption. | **PASS** |
| a Jacobian is available for the uniqueness test | capability→producer — preferred: **P2 leaf ε = #6672** (forward-mode dual numbers over `CompiledExpr`; there is no autodiff in the workspace today and no external crate is adopted). Unconditional fallback: local finite differences. Soundness is unaffected either way — a failed uniqueness test simply keeps splitting (C8) — so the edge is deliberately left **unwired** rather than blocking γ on a sibling PRD. | **PASS** via fallback (manual) |
| angle domains do not double-count | correctness precondition (G7 INV-AD-2) — `default_bounds_for` gives `ANGLE → (−τ, τ)`, a double cover of the circle; §3.3 requires canonicalisation to one period before `Exhaustive` may be claimed, else the completeness machinery manufactures a doubled count. | **PASS** (manual) |

## δ (#6709) — verdict policy

| Capability | Evidence | Verdict |
|---|---|---|
| toleranced equality verdicts | capability→producer — `producer:task-6653` (pending, high). Without it a correctly-solved root is re-checked at exact `f64` and reported `violated` (probe-observed on five fixtures), so δ's fixtures could not be green end-to-end. **Upstream** edge. | **PASS** |
| fixtures parse | grammar-fixture — `tests/prd-gate/fixtures/{ssc_two_roots_free,ssc_two_roots_strict,ssc_single_root_free}.ri`, each `tree-sitter parse --quiet` exit 0, **0 ERROR nodes**, 2026-08-26. No novel syntax (PRD §2.3). | **PASS** |
| the asserted baseline is the measured baseline | G6 branch 3 — every "today" claim δ's signal contrasts against is **observed**, with exit codes and verbatim text in PRD §0.2, not inferred from code reading. | **PASS** |
| the silence assertion is falsifiable | G6 branch 4 (negative assertion) — BT4 asserts the non-uniqueness warning **stops** firing on `ssc_single_root_free.ri`. The warning is **observed firing there today**, so the assertion has an observable before-state and cannot pass vacuously.<br>`grep:W_SOLUTION_NOT_UNIQUE\|SolutionNotUnique` → `present` in `crates/reify-core/src/diagnostics.rs` | **PASS** |
| δ's signal does not depend on the ordering edge | anti-inversion — δ's fixtures carry ONE `auto` by design, so they have no axis order to permute and C6's multi-`auto` ordering claim (P1 γ **#6691**) is not on δ's critical path; it is asserted at ζ **#6711**, which is dep-wired to #6691. | **PASS** |

## ε (#6710) — refutation by subdivision

| Capability | Evidence | Verdict |
|---|---|---|
| the empty-box proof arm exists upstream | capability→producer — `producer:task-6655` delivers the whole-box empty ⇒ typed proven-infeasible arm; ε extends it to the subdivision case. **Upstream** edge. | **PASS** |
| ε's fixture is not already #6655's | **producer-extent guard (deliberate)** — `tests/prd-gate/fixtures/ssc_refuted_pair.ri` (`L == 10mm` ∧ `L == 20mm`) parses clean and **observed** today as exit 1 / `error: constraints could not be satisfied (max absolute residual: 5.00e-3)`. It is refuted by HC4 **at the root box**, so #6655 alone turns it green. It is committed as C3 baseline evidence, **not** as ε's signal. ε's own fixture — a root box HC4 does not empty, refuted only by subdivision — cannot be authored or verified before #6655's propagator exists, so **constructing it and demonstrating root-box HC4 leaves it undecided is part of ε's acceptance**. | **PASS** (manual; obligation stated on the leaf) |

## ζ (#6711) — stop discarding the alternatives

| Capability | Evidence | Verdict |
|---|---|---|
| the alternatives are produced today | capability→producer — `DimensionalSolver::solve_ranked_impl` produces `K = 2·(dim+1)` ranked candidates; both `engine_eval.rs` consumption seams `swap_remove(0)` and discard `candidates[1..]`. `RankedSolveResult` / `RankedCandidate` appear in **no crate outside** `reify-ir` / `reify-constraints` / `reify-eval`. Grep-verified. The producer exists and the consumer is absent — ζ **is** the consumer.<br>`grep:swap_remove\(0\)` → `absent` in `crates/reify-eval/src/engine_eval.rs` | **PASS** |
| the rendering surface exists | capability→producer — `reify explain` (`cmd_explain`) + `ObjectiveProvenance`, landed by `constraint-solver-completion.md` θ **#4015** / ι **#4017** (both done), wired on the production CLI path. It carries no optimality, uniqueness or completeness token today. | **PASS** |
| a dedup key is genuinely missing | signal premise — `solve_ranked_impl` documents that non-winning candidates are **not deduplicated**, so for a single-basin objective most of K are near-identical convergences of one optimum. C5's box identity is the missing key. | **PASS** |
| deterministic ordering | **DAG-direction, WIRED** — P1 leaf γ = **#6691**, real edge 6711 → 6691. | **PASS**, edge WIRED |

## η (#6712) — basin stability across warm re-solves

| Capability | Evidence | Verdict |
|---|---|---|
| warm paths exist to instrument | capability→producer — `edit_param` (`crates/reify-eval/src/engine_edit.rs`) seeds `current_values` from the prior snapshot **including previously solved autos**; reached from the GUI `set_parameter` and `crates/reify-cli/src/mcp_context.rs`. `Engine::eval_cached` is the second warm path (LSP). | **PASS** |
| the cold-only hole is real | capability→producer — `W_SOLVER_OPTIMALITY_UNPROVEN` and `detect_underdetermined` have production call sites only in `Engine::eval`, not `eval_cached`; `dispatch_merged_cluster_solve_cached` documents the consequence in-tree.<br>`grep:W_BASIN_CHANGED\|BasinChanged` → `present` in `crates/reify-core/src/diagnostics.rs` | **PASS** |
| entry-point parity | **DAG-direction, WIRED** — P1 leaf κ = **#6699**, real edge 6712 → 6699. η owns basin identity; P1 owns parity. | **PASS**, edge WIRED |

## θ (#6713) — C1 conformance

| Capability | Evidence | Verdict |
|---|---|---|
| the over-claiming sites exist | capability→producer — `SolveSpaceSolver::solve` hardcodes `unique: true` on every libslvs `Ok` arm and on the empty-autos arm (and it **is** in `SolverRegistry::production()`'s Geometric slot); `relate_solve::solve_frame` sets `unique = fully_pinned && !unknown.free`, where `fully_pinned` is local Jacobian rank. Grep-verified.<br>`grep:unique:[[:space:]]*true` → `absent` in `crates/reify-constraints/src/solvespace.rs` | **PASS** |
| θ does not fix code P2 deletes | **anti-inversion, resolved by re-scoping** — P2 leaf **ξ #6682** deletes `SolveSpaceSolver`, `GeometricPattern` and `recognize_pattern` outright and leaf **ι #6677** routes relate-solve through the unified problem. θ was re-scoped at authoring from "fix two drivers" to "C1 conformance over whatever writes `unique` after P2 ι", and is now dep-wired to **#6677**. | **PASS**, edge WIRED |
| the in-progress collision is sequenced | anti-inversion — **#6659** is in-progress with a live claimant editing `registry.rs` / `solvespace.rs`; θ is sequenced after it rather than racing it. | **PASS** |

## ι (#6715) — the composition law

| Capability | Evidence | Verdict |
|---|---|---|
| a conjunction already exists to replace | capability→producer — `SolverRegistry::solve_inner` conjoins per-component `unique` (`all_unique` / `other_unique`) and forces `unique = false` on intermediate lexicographic ranks. The §3.5 meet replaces that conjunction in place.<br>`grep:fn[[:space:]]+.*meet_completeness\|Completeness::meet` → `present` in `crates/reify-constraints/src/registry.rs` | **PASS** |
| a declined component has a verdict | seam with P2 ν **#6681** (capability-based routing, same file) — once routing can *decline* a component, the meet needs a `Completeness` for it: `Partial` carrying P2 δ **#6671**'s typed refusal reason, never `NotAttempted` (which θ retires). Stated so an implementer does not improvise one. | **PASS** (manual) |
| ι does not depend on PRD 2 ζ | anti-inversion — ι implements the meet over **components**, which exist today (multiple continuous components per model). PRD 2 ζ **#5472** is a *consumer* of the law, not a producer of ι's capability, so ι is not blocked on a pending PRD-2 leaf. | **PASS** |
| the registry collision is sequenced | anti-inversion — after **#6659** (in-progress, live claimant, same file). | **PASS** |

## λ (#6716) — envelope calibration

| Capability | Evidence | Verdict |
|---|---|---|
| the budget introduces no wall-clock bound | numeric floor / C7 — the enumeration budget is a **node count**, never seconds. `tests/infra/test_no_new_wallclock_upper_bounds.sh` therefore needs no new registration, and determinism survives a loaded machine. | **PASS** |
| the cap is measured, not guessed | G6 branch 1 — the cap value is **deliberately not asserted at authoring**; λ *is* the measurement, which is branch 1's sanctioned resolution for an otherwise-guessed threshold. The only stated neighbouring datum is the measured Nelder-Mead simplex knee (~10–15 vars; `WHOLE_MODEL_CLUSTER_DIM_CAP = 12`), used as an ordering hint, not as the bound. Enumeration and refutation get **separate** caps because refutation needs no uniqueness test and prunes harder. | **PASS** (manual) |

## κ (#6718) — docs-truth

| Capability | Evidence | Verdict |
|---|---|---|
| the doc chunk gap is real | capability→producer — `crates/reify-mcp/src/tools/chunks/constraints.md` contains **zero** uniqueness / multiple-solution content (grep-verified for `unique`, `multiple solution`, `root`, `basin`: no hits). κ is its producer.<br>`grep:more than one solution\|not unique\|multiple solutions` → `present` in `crates/reify-mcp/src/tools/chunks/constraints.md` | **PASS** |
| the exemplar carries a false claim κ corrects | G6 branch 3 — `examples/best_practices/discrete_choice.ri` states "Real branch-and-bound over discrete variables is what CP-SAT will bring". **False for its own file:** `side` is a continuous `Real` auto with no finite domain, and CP-SAT requires one via `build_variable_domain`, so the CP-SAT path can never reach it. Rung 2 serves this exemplar, not rung 1.<br>`grep:what CP-SAT will bring` → `absent` in `examples/best_practices/discrete_choice.ri` | **PASS** |
| the index edit is gated | capability→producer — `best_practices_index_matches_corpus_directory` build-gates `examples/best_practices/INDEX.md` against the corpus directory; the row edit is same-diff with the exemplar edit. | **PASS** |

## μ (#6719) — PRD close

| Capability | Evidence | Verdict |
|---|---|---|
| the terminal vocabulary and header shape are specified | capability→producer — the closed three-value terminal vocabulary (`SHIPPED` / `SUPERSEDED` / `WITHDRAWN`) and the three-part freeze header are specified in the PRD overlay; in-corpus exemplars `docs/prds/v0_6/data-carrying-enums.md` and `docs/prds/kernel-seam-contracts.md`.<br>`grep:Status.*(SHIPPED\|SUPERSEDED\|WITHDRAWN)` → `present` in `docs/prds/v0_6/solution-set-completeness.md` | **PASS** |
| the close leaf stays dispatchable | anti-inversion — μ depends on every sibling by real `add_dependency` edges; a `cancelled` sibling counts as satisfied for the edge, per the overlay's cancelled-dependency disposition. | **PASS** |

---

**No FAIL bindings. The batch is not blocked, and no cross-PRD edge remains outstanding.** Three hard edges are wired (ζ→#6691, η→#6699, θ→#6677); the fourth (γ←#6672) is soft and deliberately unwired. Bindings were re-walked against P2's decomposed leaf set on 2026-08-26 after P2 exited.
