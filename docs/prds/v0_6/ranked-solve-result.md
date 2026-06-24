# Ranked solve result — alternatives carrier + objective score + optimality status (F-result)

**Milestone:** v0_6 · **Status:** active (authored in interactive `/prd` session, 2026-06-24, under G1–G6+META) · **Approach:** B + H
**Cluster:** `cost-optimisation` (foundation). One of two foundations the geometry-dependent cost-min outer loop (`material-waste-cost-minimisation.md`, M-WASTE) and the discrete enumeration harness (`discrete-cost-minimisation.md`, PRD 2) are gated on. Sibling foundation: `realization-cache-input-cone-rekey.md` (F-cache).

---

## §0 — Purpose and scope

Today a constraint solve returns **exactly one** solution and **discards** how good it is:

- `SolveResult::Solved { values, unique }` (`crates/reify-ir/src/constraint.rs:206`) carries a single `HashMap<ValueCellId, Value>` — no notion of "here are the top-N candidates with their objective scores".
- Convergence quality is **computed but thrown away**: `crates/reify-constraints/src/solver.rs:890` already evaluates `termination_reason == Some(TerminationReason::MaxItersReached) && has_objective`, then logs it to `tracing::debug!` only (`solver.rs:994–1000`) — the explicit comment there says it is *not* propagated "to avoid a breaking API change across 6+ consumer crates." So a user whose cost objective parked on an un-converged local point is told nothing.

Any search that produces a **set** — a discrete enumeration (CP-SAT), a multi-start continuous sweep, or M-WASTE's geometry candidate loop — needs a carrier for "top-N candidates, each with an objective score, plus a proven-optimal-vs-best-found flag." This PRD ships that carrier and, as its own self-contained user-observable slice, **stops discarding the optimality status** the solver already computes.

This is a **public-API shape decision** (≥ 6 crates depend on `SolveResult`) → **design-first (B + H)**: §3 is the contract pinning the result type's shape and the back-compat guarantee; §4 is the two-way boundary-test sketch.

### §0.1 — What this is NOT (scope boundaries, resolved 2026-06-24)

- **NOT a new solver / search algorithm.** This PRD ships a *carrier* + an *optimality channel*. It does **not** add multi-start, enumeration, or any new candidate-generating search. The only producer that fills more than one candidate today is a future consumer (PRD 2 / M-WASTE); this PRD's own producer fills a **size-1** ranking (the single-solution case) and labels its optimality honestly.
- **NOT a change to `SolveResult` or `ConstraintSolver::solve()`.** Both are **frozen** (invariant I1). The carrier is a *sibling* type; the trait gains a *defaulted* method. Every existing caller compiles and behaves byte-for-byte identically — this is the G6 branch-3 (back-compat) requirement, asserted in the boundary tests.
- **NOT discrete / enumeration candidate generation.** CP-SAT enumeration that fills a true top-N ranking and claims `ProvenOptimal` is **`discrete-cost-minimisation.md`** (PRD 2), which *overrides* `solve_ranked`. This PRD ships the trait method + default; PRD 2 owns the override.
- **NOT the geometry outer loop.** The candidate-sweep eval mode that assembles a ranking from repeated geometry evals is **M-WASTE** (`material-waste-cost-minimisation.md`). M-WASTE *consumes* this carrier; it does not live here.

---

## §1 — Consumer (G1)

Every mechanism this PRD introduces has a named consumer; the integration mechanism additionally has an **on-main user-observable surface** so the carrier is not a producer-orphan waiting on the (deferred) outer-loop consumers.

| Mechanism | Consumer |
|---|---|
| `RankedSolveResult` carrier type (+ `RankedCandidate`, `OptimalityStatus`) in reify-ir | **On-main PRDs by name:** `material-waste-cost-minimisation.md` §Sketch row 1 ("returns ranked alternatives via F-result") and `continuous-cost-minimisation.md` §0.1 ("a ranked-result carrier → … over `ranked-solve-result.md`"). M-WASTE's milestone task **4787** lists F-result's terminal task as a precondition. |
| `ConstraintSolver::solve_ranked()` trait method (default impl + `DimensionalSolver` override surfacing optimality) | **On-main user surface:** `reify eval` / `reify check` — the engine objective-solve path (`engine_eval.rs:2897`) calls it and emits `W_SOLVER_OPTIMALITY_UNPROVEN` when a feasible point was found but optimality was not proven. **Forward consumer:** PRD 2's CP-SAT enumeration override. |
| `W_SOLVER_OPTIMALITY_UNPROVEN` diagnostic | **End user** of `reify eval`/`reify check`: a designer who writes a `minimize`/`maximize` objective and whose solve hit the iteration limit now *sees* that the result may not be globally optimal, instead of a silent park. |

**Engine-integration sub-check (G1).** The solver-side mechanism plugs into the catalogued **§3.5 ConstraintSolver** seam (`docs/prds/v0_3/engine-integration-norm.md`). It does **not** introduce a new seam — it adds a *defaulted method* to the existing `ConstraintSolver` trait, and the on-main consumer is the existing `Engine::eval` objective-solve call site. No orphan-producible `pub fn` in a `kernel-*` crate. A one-line norm-catalog note for the new method is a companion correction task (§7 δ).

---

## §2 — Background & substrate (verified in-tree 2026-06-24)

- **`SolveResult`** — `crates/reify-ir/src/constraint.rs:204–232`: `Solved { values, unique }` / `Infeasible { diagnostics }` / `NoProgress { reason }`.
- **`ConstraintSolver` trait** — `constraint.rs:366`: `fn solve(&self, &ResolutionProblem) -> SolveResult`. Impls: `DimensionalSolver` (Nelder-Mead, `solver.rs:1167`), `SolverRegistry` (`registry.rs:96` + `solve_lexicographic`), `CpSatSolver` (`cpsat.rs:199`), the SolveSpace adapter (`solvespace.rs:875`), and the relation solver (`relate_solve.rs`).
- **Production consumers** (non-test) of `SolveResult` variants: `reify-eval/src/engine_eval.rs` (2900, 4015), `engine_edit.rs` (1243, 3069), `concurrent.rs` (436), `relate_solve.rs` (486). **Decisive back-compat fact:** the `engine_eval`/`engine_edit`/`concurrent` matches destructure `Solved { values, unique }` **without `..`** — so *adding a field to `Solved` would not compile*. A **sibling type** is the only strictly back-compatible shape (the chosen design).
- **Optimality already computed, discarded:** `solver.rs:890` derives the MaxIters-with-objective flag; `solver.rs:994–1000` documents the deliberate non-propagation. This PRD captures it into `OptimalityStatus` in the `solve_ranked` override — no re-derivation, the flag is already in hand.
- **Diagnostic channel:** `DiagnosticCode` enum at `crates/reify-core/src/diagnostics.rs:156`; `Diagnostic::warning(msg).with_code(code)` is the existing idiom (`engine_eval.rs:1170,1177`); warnings pushed into the eval-path `diagnostics` vec flow to `EvalResult` → `reify eval`/`check` CLI output. `continuous-cost-minimisation.md` already plans a robustness-floor info diagnostic over the same surface — this PRD reuses it.
- **Objective vocabulary** (reused, not extended): `ObjectiveSet` / `ObjectiveSense` (`constraint.rs:68`, `pub`) / `eval_objective_set` (`reify-constraints`) — already shipped by `constraint-solver-completion.md`. The objective *score* is the scalar `eval_objective_set` returns (the folded penalty cost the Nelder-Mead solver minimises).
- **Grammar:** the user-observable leaf's fixture uses **existing** `minimize` syntax (`MinimizeDecl`, shipped end-to-end per `continuous-cost-minimisation.md`). **No novel `.ri` syntax — G3 grammar gate N/A.**

---

## §3 — Contract (B + H)

The seam is a Rust type + a trait method. An architect implementing the producer side should need no further discussion.

### §3.1 — Carrier types (new, in `reify-ir`; additive only)

```rust
/// Optimality status of a ranked solve.
pub enum OptimalityStatus {
    /// The ranking is provably globally optimal over the producer's declared search
    /// space (e.g. exhaustive discrete enumeration). MAY only be set by a producer
    /// that actually proved it (invariant I3).
    ProvenOptimal,
    /// Best candidate(s) found within budget; optimality NOT proven. `reason` carries
    /// the termination cause (e.g. "iteration limit reached", "candidate budget exhausted").
    BestFound { reason: String },
    /// No objective governs the solve — "optimal" does not apply (feasibility-only).
    FeasibilityOnly,
}

/// One candidate in a ranked solve result.
pub struct RankedCandidate {
    /// Resolved auto-param values for this candidate (same shape as SolveResult::Solved.values).
    pub values: std::collections::HashMap<ValueCellId, Value>,
    /// The ranking scalar at this candidate; LOWER is better (the producer normalises to
    /// a minimisation convention — see I2). `None` only for a feasibility-only solve.
    /// The producing path documents the measured quantity (folded solver cost for
    /// solver-produced rankings; the user objective expression value for outer-loop rankings).
    pub objective_score: Option<f64>,
    /// Whether this candidate is uniquely determined (carries SolveResult::Solved.unique semantics).
    pub unique: bool,
}

/// Result of a ranked / multi-candidate solve. SIBLING to SolveResult — SolveResult is unchanged.
pub enum RankedSolveResult {
    /// One or more candidates, ordered best-first (index 0 is the selected optimum).
    Ranked {
        candidates: Vec<RankedCandidate>,     // non-empty; ordered best-first (I2)
        optimality: OptimalityStatus,
    },
    /// No feasible candidate exists.
    Infeasible { diagnostics: Vec<Diagnostic> },
    /// The search made no progress (e.g. iteration limit with no feasible point).
    NoProgress { reason: String },
}
```

### §3.2 — Trait method (defaulted; existing impls untouched)

```rust
pub trait ConstraintSolver: Send + Sync {
    fn solve(&self, problem: &ResolutionProblem) -> SolveResult;   // UNCHANGED — I1

    /// Solve and return a ranked carrier with an optimality status.
    /// DEFAULT: structurally lift `self.solve()` into a size-1 ranking (I5):
    ///   Solved{values,unique} -> Ranked{ candidates:[{values, objective_score:None, unique}],
    ///                                     optimality: if problem.objective.is_some()
    ///                                        { BestFound{reason:"solver does not report optimality"} }
    ///                                        else { FeasibilityOnly } }
    ///   Infeasible{d} -> Infeasible{d};  NoProgress{r} -> NoProgress{r}.
    /// Override to populate a real objective_score and a tighter OptimalityStatus.
    fn solve_ranked(&self, problem: &ResolutionProblem) -> RankedSolveResult {
        /* default lift as documented above */
    }
}
```

`DimensionalSolver` (Nelder-Mead) **overrides** `solve_ranked`: it computes `objective_score` via `eval_objective_set` at the solution and sets `optimality` = `FeasibilityOnly` (no objective) or `BestFound { reason }` (objective present — Nelder-Mead is derivative-free and budget-bounded; it **never** claims `ProvenOptimal`), reading the MaxIters-vs-converged `reason` from the termination flag already at `solver.rs:890`.

### §3.3 — Invariants (the contract)

- **I1 — back-compat freeze.** `SolveResult` and `ConstraintSolver::solve()` are unchanged. Every pre-existing caller compiles and behaves byte-for-byte identically. (Boundary tests B5/B6.)
- **I2 — rank order.** In `Ranked`, `candidates` is **non-empty** and ordered **best-first by ascending `objective_score`** (lower = better; producers normalise maximisation to this convention — the solver already folds `σ(Maximize) = −1`). Feasibility-only rankings (`objective_score: None`) are size-1; no ordering claim.
- **I3 — optimality honesty.** `ProvenOptimal` may be set **only** by a producer that proved global optimality over its declared search space. A derivative-free / budget-truncated solve (`DimensionalSolver`) MUST use `BestFound`. `FeasibilityOnly` iff no objective governs the solve.
- **I4 — score↔objective coherence.** `objective_score.is_some()` iff an objective governed the solve. The score is the scalar `eval_objective_set` returns at that candidate (folded penalty cost) for solver-produced rankings.
- **I5 — default-method fidelity.** The default `solve_ranked` lifts `solve()` losslessly: resolved values, `unique`, and the `Infeasible`/`NoProgress` mapping are preserved exactly; only `objective_score` (None) and `optimality` (conservative) are synthesised. An un-overridden solver therefore stays correct, merely uninformative about optimality.

---

## §4 — Boundary-test sketch (B + H, two-way)

The integration-gate leaf (§7 γ) names this table as its observable signal.

| # | Side | Scenario | Preconditions | Postconditions (asserted) |
|---|---|---|---|---|
| B1 | producer (DimensionalSolver) | objective solve that hits the iteration limit | `minimize` objective + auto param + low iteration budget so MaxIters fires | `solve_ranked` → `Ranked{ candidates:[1], optimality: BestFound{ reason ~ "iteration limit" } }`; `candidates[0].objective_score.is_some()` |
| B2 | producer (DimensionalSolver) | feasibility-only solve (no objective) | auto param, constraints, **no** objective | `Ranked{ candidates:[1], optimality: FeasibilityOnly }`; `objective_score == None` |
| B3 | producer (default impl) | a solver that does **not** override `solve_ranked` (e.g. SolveSpace adapter) | any feasible problem | default lift → `Ranked{ candidates:[1] }`; `candidates[0].values` equals `solve().values`; `unique` preserved |
| B4 | consumer (engine) | `reify eval` on a fixture whose objective solve hits MaxIters | B1 fixture as an `examples/*.ri` | CLI output contains `W_SOLVER_OPTIMALITY_UNPROVEN`; **resolved values are byte-identical** to the pre-change baseline (I1 regression guard) |
| B5 | back-compat | existing `SolveResult` consumers compile unchanged | none | reify workspace builds with **zero** edits to the `engine_eval`/`engine_edit`/`concurrent` `Solved{values,unique}` matches |
| B6 | back-compat | a converged objective solve emits **no** optimality warning | objective solve that converges below the iteration budget | `reify eval` output contains **no** `W_SOLVER_OPTIMALITY_UNPROVEN`; resolved values unchanged (no false-positive) |

---

## §5 — Pre-conditions for activating

**None — this PRD is immediately decomposable and shippable on main.** It uses only existing substrate (frozen `SolveResult`, the §3.5 trait, the shipped objective vocabulary, the existing diagnostic channel, existing `minimize` grammar). It is itself a precondition for M-WASTE and PRD 2, not gated by them.

---

## §6 — Cross-PRD relationship (G4)

| Other PRD | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `material-waste-cost-minimisation.md` (M-WASTE, task 4787) | consumes | `RankedSolveResult` carrier (outer loop assembles & returns it) | **this PRD** owns the carrier; M-WASTE owns the outer loop | **queued** — task 4787 dep-wired on this PRD's terminal task (§7 γ) at decompose |
| `discrete-cost-minimisation.md` (PRD 2, unauthored) | consumes | `ConstraintSolver::solve_ranked` (CP-SAT enumeration **override**) + carrier | **this PRD** owns the trait method + default + carrier; **PRD 2** owns its override | **future** — PRD 2 not yet authored; no reciprocal-ownership ambiguity (override clearly belongs to the enumeration PRD) |
| `continuous-cost-minimisation.md` | references | F-result named in §0.1 as PRD 2's substrate | this PRD (prose ref only; no code seam — that PRD ships independently of this one) | **wired** (prose) |
| `engine-integration-norm.md` §3.5 | extends | `ConstraintSolver` trait gains the defaulted `solve_ranked` method | **this PRD** (non-breaking extension of the catalogued trait) | **queued** — one-line §3.5 catalog note is companion correction task §7 δ |

No new contested-ownership pair is introduced (checked against `phase-3-breadcrumb-map.md` §3). The §3.5 seam's contract owner stays `kinematic-constraints-*`; this PRD only extends the trait surface and notes it.

---

## §7 — Decomposition plan

B + H shape: foundation intermediates (α, β) feed one user-observable integration-gate leaf (γ); δ is the companion correction task. Greek labels; task IDs assigned at decompose.

- **α — `RankedSolveResult` / `RankedCandidate` / `OptimalityStatus` carrier types (reify-ir).**
  Modules: `reify-ir` (`constraint.rs` or a new `ranked.rs` re-exported from `lib.rs`).
  Intermediate — **unlocks β, γ**; consumer: §7 β/γ and the M-WASTE/PRD 2 outer loops.
  Signal (intermediate): the three types are `pub` and re-exported from `reify-ir`; β/γ depend on them.

- **β — `solve_ranked` trait method (default in reify-ir) + `DimensionalSolver` override (reify-constraints).**
  Modules: `reify-ir` (trait `constraint.rs:366`), `reify-constraints` (`solver.rs` override reading the `solver.rs:890` termination flag; `eval_objective_set` for the score).
  Intermediate — **unlocks γ**; consumer: §7 γ (engine call site) and PRD 2's override.
  Signal (intermediate): `cargo build` green with no edits to the other four `ConstraintSolver` impls (default covers them — I5); β's behaviour is exercised by γ's B1/B2/B3 boundary tests.
  Depends on: α.

- **γ — engine objective-path: emit `W_SOLVER_OPTIMALITY_UNPROVEN` via `solve_ranked` (integration-gate LEAF).**
  Modules: `reify-core` (new `DiagnosticCode::W_SOLVER_OPTIMALITY_UNPROVEN`), `reify-eval` (`engine_eval.rs:2897` — when `problem.objective.is_some()`, call `solve_ranked`; take `candidates[0].values` as the resolved values; on `optimality == BestFound` push the warning into the eval `diagnostics` vec), `examples/` (a fixture whose objective solve deterministically hits MaxIters).
  **LEAF — user-observable signal:** `reify eval examples/solver_optimality_unproven.ri` prints `W_SOLVER_OPTIMALITY_UNPROVEN` AND resolves to the same values as the pre-change baseline (boundary sketch §4 rows B1, B4, B5, B6). Consumer: `reify eval`/`check` end user.
  Depends on: α, β.

- **δ — norm §3.5 catalog note for `solve_ranked` (companion correction).**
  Modules: `docs/prds/v0_3/engine-integration-norm.md` (§3.5).
  Signal (doc): §3.5 gains a one-line note that the `ConstraintSolver` trait also exposes `solve_ranked → RankedSolveResult` (default lifts `solve()`), so the catalog stays honest.
  Depends on: α (the type names to cite). Out-of-batch correction; not blocking γ.

**Back-edge (post-decompose):** `add_dependency(4787 → γ)` so M-WASTE's milestone is gated on this PRD's terminal task.

---

## §8 — Out of scope for this PRD

- Multi-start / enumeration / any new candidate-generating search (→ PRD 2 for discrete; M-WASTE for geometry sweep). This PRD's own producer fills size-1 rankings only.
- CP-SAT `solve_ranked` override that fills a true top-N and claims `ProvenOptimal` (→ PRD 2).
- The geometry candidate-sweep eval mode that assembles a ranking from repeated evals (→ M-WASTE).
- Rerouting the **edit** / **concurrent** solve paths (`engine_edit.rs`, `concurrent.rs`) through `solve_ranked` — γ reroutes only the main `engine_eval` objective path (the cost-min path). The others keep `solve()` (no objective-optimality surface needed there yet); a follow-up may extend them.
- Surface syntax for "give me the top N" — there is no `.ri` syntax change here; the carrier is internal IR consumed by Rust-side outer loops.
- Any change to `SolveResult`, `ConstraintSolver::solve()`, or the objective vocabulary (`ObjectiveSet`/`ObjectiveSense`/`eval_objective_set`).

---

## §9 — Open questions (tactical — deferred, not design-level)

1. **Fixture mechanism for deterministically hitting MaxIters.** γ's leaf needs an objective solve that reliably terminates on the iteration limit so `BestFound` fires. **Suggested resolution:** a tight per-solve iteration budget on a mildly ill-conditioned/flat objective; if the budget is not externally settable from a fixture, use a known slow-converging objective expression. A converged fixture simply fails the RED leaf (no warning) — a *detectable* failure, never a silent false-green. Decide during γ.
2. **Module placement of the carrier** — `constraint.rs` alongside `SolveResult` vs a new `reify-ir/src/ranked.rs`. **Suggested resolution:** new `ranked.rs` re-exported from `lib.rs`, to keep `constraint.rs` focused. Decide during α.
3. **Exact severity of the optimality diagnostic** — `W_*` warning vs info-level. **Suggested resolution:** `W_*` (a possibly-non-optimal result the user should know about), matching the `continuous-cost-minimisation.md` robustness-floor diagnostic's level. Decide during γ.
4. **`reason` string vocabulary for `BestFound`** — free-form vs a small enum. **Suggested resolution:** free-form `String` now (mirrors `NoProgress.reason`); promote to an enum only if a consumer needs to branch on it. Decide during β.
