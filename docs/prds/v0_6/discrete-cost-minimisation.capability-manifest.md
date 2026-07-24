# Capability manifest — discrete cost minimisation (PRD 2)

Mechanizes G3 + G6 per leaf for `docs/prds/v0_6/discrete-cost-minimisation.md`.
Each binding maps a leaf's asserted capability to **executed** evidence, with a
PASS verdict; any FAIL blocks the batch. Authored at decompose time, 2026-07-24.

**Leaf task IDs:** stamped into the YAML sidecar
(`discrete-cost-minimisation.capability-manifest.yaml`) by `commit_planning`.

**Probe environment:** `target/debug/reify` (debug binary built 2026-07-22 —
solver/eval anchors re-verified unchanged on main `0d70ef1d5b` 2026-07-24);
`tree-sitter` from `tree-sitter-reify/`; committed fixtures under
`docs/prds/v0_6/fixtures/discrete_*.ri`.

---

## Scope note — substrate vs deliverable (why no FAILs)

Decompose-time verification asserts only the **assumed substrate** (landed
F-result carrier + dispatch, the CP-SAT solver + its domain arms, the classifier,
the `@solver_hint` compile chain, PRD 1's Money machinery) plus the **baseline
bugs** each RED signal repairs — never the tasks' own deliverables, which by
definition do not exist yet. The typed-diagnostic assertions (unbounded `Int`,
unresolvable catalog) are δ's **own deliverables** (G6 branch 4: the rejection
mechanism is introduced by the task that asserts it), verified at δ's leaf
signal, not as substrate probes.

## Numeric note (G6 branches 1/2)

No accuracy-bound or exactness premises exist in this PRD. Discrete assertions
(`n=4`, exact `Bool`s, catalog membership) are **combinatorial identities** —
enumeration either finds the assignment or it doesn't; the only numeric
assertions are solver-tolerance-scoped (`a=6, b=4`, `t≈3.0` on 2-var linear /
box-constrained problems well inside `DimensionalSolver`'s demonstrated
capability — `objective_set_weighted.ri` precedent). No floor exposure.

## Grammar evidence (anti-mismatch)

No novel syntax. All 8 committed fixtures parse with 0 ERROR nodes
(`tree-sitter parse --quiet`, 2026-07-24): `discrete_let_cont`,
`discrete_int_auto`, `discrete_mixed`, `discrete_set_hint`,
`discrete_enum_auto`, `discrete_balance_inline`, `discrete_balance_lets`,
`discrete_balance_min`.

**Semantic-substrate wrinkle (captured 2026-07-24):**
`@solver_hint("discrete_set", <local-let ident>)` is compile-REJECTED
(`error: unknown selector kind '@solver_hint'`, exit 1); the registered-stdlib
surface (`structure def` + `standard_bolt_lengths`) checks clean (exit 0).
v1 scopes catalogs to the stdlib surface (PRD §2.3/§3.7/§9).

---

## Per-leaf capability bindings

### α — let-tracing transitive closure + shared per-trial fold

| Capability | Evidence | Verdict |
|---|---|---|
| Transitive-closure builder exists & is production-wired | `grep`: `fn build_dependent_cells` `engine_eval.rs:1553`; called from `build_solver_problem` paths `engine_eval.rs:1842/2179` (not test-only) | **PASS** |
| `ResolutionProblem.dependent_cells` field + post-solve consumer exist (#5188) | `grep`: `materialize_dependent_cells` `engine_eval.rs:1684`; registry gap α fills: `dependent_cells: Vec::new()` `registry.rs:244` | **PASS** |
| Baseline bug is real (signal premise): let-indirected constraints dropped | probe `reify eval fixtures/discrete_let_cont.ri` → exit 0, `W_UNDERDETERMINED … not touched by any constraint`, all values `undef` (captured 2026-07-24) | **PASS** (bug confirmed) |
| 2-var linear resolve capability (post-fix values `a=6, b=4`) | shipped `DimensionalSolver` precedent (`objective_set_weighted.ri` calibration, PRD 1 §4); within-tolerance assertion, no new bound | **PASS** |

### β — CP-SAT honest enumeration core (`solve_all` + cap-2 unique + `solve_ranked` argmin)

| Capability | Evidence | Verdict |
|---|---|---|
| Reference implementation exists (adapt, don't invent) | spike commit `75cf3b4d19` on `spike/discrete-solve-cpsat`: `solve_all`/`SolveAllResult` + 3 unit tests, e2e 0.46 s | **PASS** |
| Objective scoring fn available to the override | `grep`: `eval_objective_set` (`reify-constraints`, shipped by constraint-solver-completion; F-result I4 uses the same scalar) | **PASS** |
| Override slot + carrier landed & reachable (anti-orphan) | `grep`: `ranked.rs` types; defaulted `solve_ranked` on trait; **registry per-component dispatch** `registry.rs:280` (#5016) + engine objective call site `engine_eval.rs:2897` (#4804) | **PASS** |
| `unique` lie baseline (signal premise) | `grep`: `unique: true` hardcoded `cpsat.rs:202/243` | **PASS** (bug confirmed) |
| Warning channel behaves correctly once optimality is honest | #4804: engine emits `W_SOLVER_OPTIMALITY_UNPROVEN` **iff** `BestFound` → `ProvenOptimal` silences it with zero engine edits | **PASS** |

### γ — default-ON wiring + `DiscreteFirstFallback` (all-discrete arm)

| Capability | Evidence | Verdict |
|---|---|---|
| Both production call sites exist (consumer surfaces) | `grep`: `SolverRegistry::production()` `reify-cli/src/main.rs:1319`, `gui/src-tauri/src/engine.rs:1981` | **PASS** |
| Registry slots exist, currently empty (the gap γ fills) | `grep`: `logical: None` / `fallback: None` `registry.rs:42-43`; CrossDomain arm `registry.rs:90` | **PASS** |
| Classifier routes natural Bool+numeric to CrossDomain (why the fallback slot is required) | `grep`: `has_numeric && has_logical → CrossDomain` (`classifier.rs` `into_domain`) | **PASS** |
| Natural-formulation baselines (signal premises) | probes: `discrete_balance_inline.ri` → exit 1 misleading residual; `discrete_balance_lets.ri` → exit 0, autos `undef` (captured 2026-07-24) | **PASS** (bugs confirmed) |
| CP-SAT solves the wired case (achievability) | spike e2e: n1 hexagon balance solved in 0.46 s, exact Bools, flag-off byte-identical | **PASS** |

### δ — discrete domain channel (`Int` bound-mining + `discrete_set` → `AutoParam.domain`)

| Capability | Evidence | Verdict |
|---|---|---|
| CP-SAT `Int` arm exists, needs only bounds | `grep`: `build_variable_domain` Int arm + `MAX_INT_DOMAIN = 1000` (`cpsat.rs:14/49-88`); `bounds: None` always (`engine_eval.rs:1439`) | **PASS** |
| Constraint-mining precedent in the same fn | `grep`: Enum arm scans constraint literals (`cpsat.rs:89-115`) | **PASS** |
| Compiler-side hints present; IR seam absent (the gap δ fills) | `grep`: `ValueCellDecl.solver_hints` `reify-compiler/src/types.rs:1125`; **zero** matches for `SolverHint` in `reify-ir`/`reify-constraints`/`reify-eval` src (M-008 re-verified 2026-07-24) | **PASS** |
| Stdlib catalog surface validates today | probe: `structure def` + `standard_bolt_lengths` → `reify check` exit 0; local-let catalog → exit 1 (out of scope, PRD §9); stdlib source `crates/reify-compiler/stdlib/standard_stock.ri` | **PASS** |
| `Int`-auto baseline failure (signal premise) | probe `discrete_int_auto.ri` → exit 1, "not uniquely determined", `undef` (captured 2026-07-24) | **PASS** (bug confirmed) |
| Typed no-finite-domain / unresolvable-catalog diagnostics | δ's **own deliverable** (G6 branch-4 producer = this leaf); asserted by δ's RED tests, not a substrate probe | **PASS** (in-set) |

### ε — integration gate (CI examples + harness e2e)

| Capability | Evidence | Verdict |
|---|---|---|
| Every asserted capability is upstream (anti-inversion) | DAG: ε ← {γ (wiring, honest solve), δ (domains)} ← β ← α; no capability owed by a task depending on ε | **PASS** |
| CI harness target exists (no new standalone test binary) | `crates/reify-eval/tests/harness_engine.rs` (harness-layout ratchet honoured; drift-guard registrations N/A — no new `tests/infra/*.sh`, no wall-clock assertions) | **PASS** |
| Money objective over an auto `Length` is shipped idiom | `examples/continuous_cost_min.ri` on main (PRD 1 β #4790); ε's stock example = same idiom over a discrete domain | **PASS** |
| Argmin-flip observable is meaningful (signal premise) | spike n3/n3b: flipped objective → same config today (silent ignore, captured); post-δ/β the flip must flip — a two-sided observable | **PASS** |

### ζ — mixed enumerate×residual outer loop

| Capability | Evidence | Verdict |
|---|---|---|
| Mixed baseline failure (signal premise) | probe `discrete_mixed.ri` → exit 1, `constraints could not be satisfied (max absolute residual: 1.00e0)` (captured 2026-07-24) | **PASS** (bug confirmed) |
| Inner continuous solve + Money machinery landed | `grep`: robustness floor `solver.rs:487` (#4789) in `DimensionalSolver` assembly → inherited by construction on the inner solve (PRD §3.8) | **PASS** |
| CrossDomain dispatch slot exists | `grep`: `fallback` slot + `registry.rs:90` CrossDomain arm | **PASS** |
| Always-`BestFound` honesty on mixed (never `ProvenOptimal`) | design invariant D2 (F-result I3 refinement) — asserted by ζ's boundary tests B9/B10; no substrate claim | **PASS** (in-set) |
| Expected optimum `(up=true, t≈3.0)` is correct | hand-check: up=true ⇒ t∈[3,10], min t → 3; up=false ⇒ t∈[5,10] → 5; argmin = (true, 3.0). Combinatorial + linear, no floor exposure | **PASS** |

### η — companion docs

| Capability | Evidence | Verdict |
|---|---|---|
| Spec currently claims PRD 2 unauthored (the row η fixes) | `grep`: "queued and not yet authored" `docs/reify-language-spec.md` (~§2285 Deferred-capabilities block) | **PASS** |
| Cross-PRD rows to update exist | F-result §6 PRD-2 row ("future — PRD 2 not yet authored"); M-WHOLE §8 PRD-2 row ("future") | **PASS** |

### θ — `[MILESTONE]` solutions() bookmark

No code premises (design-first trigger task; `execution_class: decision`).
Substrate it will consume (`solve_all`, mixed enumeration, honesty channels) is
delivered by its hard deps ε/ζ — DAG-direction verified (producers upstream).
