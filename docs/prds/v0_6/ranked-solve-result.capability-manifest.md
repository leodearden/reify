# Capability manifest — `ranked-solve-result.md` (F-result)

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/ranked-solve-result.md`. Built at decompose 2026-06-24; every binding grep-verified against `main` (HEAD at author time). **No FAIL binding → batch is clear to queue.**

Leaf/intermediate classification of the §7 decomposition:
- **α** — carrier types (reify-ir): **intermediate** (unlocks β, γ).
- **β** — `solve_ranked` trait method + `DimensionalSolver` override: **intermediate** (unlocks γ).
- **γ** — engine objective-path reroute + `W_SOLVER_OPTIMALITY_UNPROVEN` + fixture: **LEAF** (user-observable).
- **δ** — norm §3.5 catalog note: **doc-correction leaf** (signal = committed prose).

Only γ carries runtime-capability assertions; α/β are pure additive Rust foundations whose "signal" is that γ's boundary tests exercise them. δ is doc-only.

---

## γ — `reify eval`/`check` emits `W_SOLVER_OPTIMALITY_UNPROVEN` on a MaxIters objective solve

**Signal:** `reify eval examples/solver_optimality_unproven.ri` prints `W_SOLVER_OPTIMALITY_UNPROVEN` AND resolves to byte-identical values vs the pre-change baseline (§4 rows B1, B4, B5, B6).

| Capability asserted | Check | Evidence | Verdict |
|---|---|---|---|
| `RankedSolveResult` / `RankedCandidate` / `OptimalityStatus` exist & `pub` in reify-ir | Capability→producer (anti-orphan) + DAG-direction | `producer:task-α` — **upstream** of γ in this batch; α adds the types to reify-ir and re-exports from `lib.rs` | **PASS** |
| `ConstraintSolver::solve_ranked()` returns an `OptimalityStatus` populated from the solver's MaxIters flag | Capability→producer + DAG-direction | `producer:task-β` — **upstream**; β overrides `DimensionalSolver::solve_ranked` reading the flag already computed at `grep:crates/reify-constraints/src/solver.rs:890` (`MaxItersReached && has_objective`) and scoring via `eval_objective_set` | **PASS** |
| Engine objective-solve call site to reroute exists on main | Wired-on-main | `grep:crates/reify-eval/src/engine_eval.rs:2897` `.solve(&problem)`; objective present at `engine_eval.rs:2985` (`problem.objective.as_ref()`) | **PASS** |
| Warning-diagnostic channel: `Diagnostic::warning(..).with_code(..)` pushed into the eval `diagnostics` vec surfaces to `reify eval`/`check` | Wired-on-main | `grep:crates/reify-eval/src/engine_eval.rs:1170` (existing `Diagnostic::warning` idiom on the eval path) → flows to `EvalResult` → CLI; same surface `continuous-cost-minimisation.md` reuses for its robustness-floor diagnostic | **PASS** |
| `DiagnosticCode::W_SOLVER_OPTIMALITY_UNPROVEN` enum value | Capability→producer (field-population: a real warning, not a placeholder) | `producer:task-γ` — γ adds the variant to the `DiagnosticCode` enum at `grep:crates/reify-core/src/diagnostics.rs:156` and emits a real warning string (non-sentinel) on the production eval path | **PASS** |
| The fixture's objective solve **observably fires** the warning (negative-assertion-shaped) | Rejection-mechanism / achievability | The mechanism is **built by γ itself** (not an asserted-existing rejection), so this is `producer:task-γ` + achievability: the MaxIters branch at `solver.rs:890` is reachable by construction (tight per-solve iteration budget / mildly ill-conditioned objective). A converged fixture simply leaves γ's RED leaf failing — a **detectable** miss, never a silent false-green. **Not** `rejection-absent` (no existing mechanism is being asserted) | **PASS** |
| Fixture grammar (`minimize <Money/cost-expr>`) parses | Grammar reality (anti-mismatch) | **No novel syntax** — `minimize`/`MinimizeDecl` ships end-to-end (`continuous-cost-minimisation.md` §0; `constraint-solver-completion.md`). G3 grammar gate **N/A**; reuse a shipped `minimize` fixture shape | **PASS** |
| Numeric accuracy floor | Numeric floor (anti-floor) | **N/A** — the leaf asserts a diagnostic string + value byte-identity, **no** absolute numeric tolerance. G6 numeric branches (1,2) inert | **PASS** |
| Back-compat freeze (I1): `SolveResult` + `ConstraintSolver::solve()` unchanged; the no-`..` production matches compile untouched | Capability→producer (anti-regression) | `grep` evidence the production consumers destructure `Solved { values, unique }` **without `..`** — `crates/reify-eval/src/engine_eval.rs:2900,4015`, `engine_edit.rs:1243,3069`, `concurrent.rs:436` — so a **sibling** carrier type (α) touches none of them. Asserted by §4 boundary tests B5 (workspace builds with zero edits to those matches) + B6 (no false-positive warning on a converged solve) | **PASS** |

---

## α / β / δ — intermediate & doc-correction bindings (no runtime-capability assertion)

| Task | Kind | Binding | Verdict |
|---|---|---|---|
| α | intermediate (carrier types) | additive `pub` types in reify-ir; consumers β, γ are **downstream** (DAG-direction correct). No existing capability asserted | **PASS** |
| β | intermediate (trait method + override) | defaulted trait method (existing 4 impls inherit the default → `cargo build` green with zero edits to `cpsat.rs`/`solvespace.rs`/`registry.rs`/`relate_solve.rs`); `DimensionalSolver` override reads the existing `solver.rs:890` flag (`grep`-confirmed present). Consumer γ downstream | **PASS** |
| δ | doc-correction leaf | one-line `engine-integration-norm.md` §3.5 note citing the α type names; signal = committed prose. No code capability | **PASS** |

**Aggregate:** 0 FAIL / 0 UNPROVABLE. Batch clear to queue.
