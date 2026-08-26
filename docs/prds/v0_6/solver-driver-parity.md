# Solver driver parity — solve everywhere

**Milestone:** v0_6 · **Status:** active — contract PRD · **Approach:** B + H

Authored 2026-08-26 in an interactive `/prd` session (Leo + Claude; groundwork by a
six-agent team). First of four solver-program PRDs (P1). Siblings, both landed the
same day: `docs/prds/v0_6/geometry-algebra-solver-unification.md` (P2, parallel) and
`docs/prds/v0_6/solver-legibility-telemetry.md` (P4), which consumes this document.

**Code anchors** verified against main `2128c3692cbb88f59b6e9edfd25ee801513423bb`
(2026-08-26). Main moves fast — cite-by-symbol; re-locate lines at implementation
time.

---

## §1 — Goal

One resolution semantics, everywhere. Today the same `.ri` file resolves its `auto`
parameters under `reify eval`, `report`, `explain` and the GUI viewport, and does
**not** resolve them under `reify check`, `reify build`, `reify test`, the LSP, the
CLI `mcp-server`, the GUI's definition-preview pane, or the GUI's own in-process
LSP. The divergence is invisible: `check` prints `No constraints violated
(1 indeterminate).` and exits 0, and `build` writes a STEP file realized from the
child's *default*, not the solved value.

After this PRD, every driver that resolves a model runs the **same** resolution:
same constraint population, same solver registry, same acceptance, same verdicts.
Drivers differ only along two declared axes — **iteration budget** and
**staleness** — and both are typed and surfaced. No driver silently declines.

User-observable on landing:

- `reify check tests/prd-gate/fixtures/driver_parity_auto_ctl.ri` resolves the
  `auto` and reports the constraint `Satisfied` — today it prints
  `No constraints violated (1 indeterminate).` and exits 0.
- `reify build` on the same fixture writes a STEP whose cylinder z-extent is
  **−10 mm** (the solved value), not −5 mm (the child default). Measured today:
  −5 mm, byte-identically across every `Rod(length: auto)` probe.
- `reify check` on an `auto` pinned only by a one-sided inequality exits nonzero
  with a coded diagnostic naming the missing bound — instead of silently resolving
  it to the 10-metre edge of the solver's hardcoded default domain.
- `reify check --strict` on a model whose `relate` block is contradictory exits
  nonzero — today it prints `All constraints satisfied.` and exits 0.
- `reify test` on a `@test` structure whose constraint reads an `auto` reports a
  real pass/fail instead of `Indeterminate`, and prints the `TestResult`
  diagnostics it currently drops on the floor.
- Two `reify eval` runs of the same file on the same machine produce
  bit-identical resolved values — a property `whole-model-objective-coupling.md`
  BT5 already claims and which is **not held today**.
- The same fixture driven through `check`, `eval`, `build`, `test`, the LSP and the
  GUI reports the same resolution, asserted by a committed differential harness.

### §1.1 — Committed evidence fixtures

Five fixtures land with this PRD. Each parses with **0 ERROR nodes** under
`tree-sitter parse --quiet` and was executed at HEAD; the baseline recorded in each
file's header is the observed output, not a prediction.

| Fixture (all under `tests/prd-gate/fixtures/`) | Pins | Measured baseline at `2128c3692c` |
|---|---|---|
| `driver_parity_auto_ctl.ri` | B1, B2 | `check` exit 0 `No constraints violated (1 indeterminate).`; `eval` solves `0.01 m`; `build` STEP z-extent −5 mm |
| `driver_parity_one_sided_auto.ri` | B4 | `eval` → `OneSided.rod.length = 10 m`, exit 0 |
| `driver_parity_two_sided_auto.ri` | B5 | `eval` → `0.01 m` — the analytic centre of `[8, 12]` |
| `driver_parity_eq_plus_ineq.ri` | §7 prereq 2 | `eval` exit 1, `max absolute residual: 5.00e-7`, value `undef` |
| `driver_parity_relate_conflict.ri` | B7 | `check` **and** `check --strict` both exit 0 `All constraints satisfied.`; `eval` exit 1 with a precise conflict diagnostic |

`driver_parity_two_sided_auto.ri` is the **discriminating negative case** for the
one-sided refusal: without it, ε could be implemented as a blanket "an
inequality-pinned `auto` is refused", which would break every legitimately
two-sided design while still passing B4.

**Registration obligation.** These are new fixtures, so committing them does not
escalate the docs gate. But the moment a leaf wires a compiled test target to read
one, that leaf's **own diff** must add the basename to `_RUST_COUPLED_RI_FIXTURES`
(`scripts/verify.sh`) — `tests/infra/test_verify_scope.sh`'s PG-DRIFT scenario
re-derives the coupled set from tracked sources and goes red otherwise. This binds
δ, ε, ρ and φ. It is a same-diff obligation, never a follow-up task: ordering a
registration after the test that needs it is the `esc-4914-162` failure.

---

## §2 — Background

### §2.1 — The inversion

On 2026-06-10 Leo resolved `esc-4458-87` by choosing SCOPE-WIDEN: keep
`configured_eval_engine(...).with_solver(production())` in `cmd_build`, update the
three `cli_build.rs` indeterminate tests, and explicitly supersede "build realizes
geometry, doesn't solve". The re-dispatched implementer — a context-free restart
with no re-plan — hit the same scope wall and committed the opposite (`37bafd9acc`,
merged `7ba73d5a78`). The surviving `cmd_build` comment's claim that "cmd_build's
solver-free posture is intentional" **is that commit's own text**, not a design
decision. Provenance, surviving records and the blast radius are recorded in
task #6631's `details`.

`reify check`'s solver-free posture has **no recorded rationale anywhere** — an
exhaustive archaeology pass found no decision either way. Its likely origin is the
M1 milestone scoping line in `docs/reify-implementation-plan.md`
("`reify-constraints`: Constraint *checking* only (no solving)"), never
re-examined after the solver landed.

The flip is therefore **already ruled**. This PRD executes a standing ruling; it
does not re-open one.

### §2.2 — What the posture actually is today

There are exactly **two** production solver-wiring sites in the workspace:

| Site | Symbol |
|---|---|
| CLI | `configured_eval_engine` (`crates/reify-cli/src/main.rs`) |
| GUI | `EngineSession::with_registered_kernel` (`gui/src-tauri/src/engine.rs`) |

Every other engine construction gets `solver: None` **by omission**. That is the
structural fact this PRD attacks: solver-free is the free, silent default, so any
implementer who constructs an `Engine` without thinking produces a driver that
silently fails to resolve. The 2026-06 inversion was not an accident of one
implementer's judgement — it was the predictable outcome of a design where the
wrong thing is the default and the right thing lives in a comment.

The corrected driver census (the prior map was wrong or incomplete in eleven
places; see §12):

| Driver | Symbol | Solver today |
|---|---|---|
| `reify eval` / `run` | `cmd_eval` → `configured_eval_engine` | **yes** |
| `reify report` | `cmd_report` → `configured_eval_engine` | **yes** |
| `reify explain` | `cmd_explain` → `configured_eval_engine` | **yes** |
| GUI viewport / edit | `EngineSession::with_registered_kernel` | **yes** |
| GUI Tauri MCP, debug server | share the session's `Arc<Mutex<EngineSession>>` | **yes** |
| `reify check` | `cmd_check` (three construction sites) | no |
| `reify build` | `cmd_build` | no |
| `reify test` | `build_test_engine` (`crates/reify-eval/src/test_runner.rs`) | no |
| LSP | `EvalState::new`, `AnalysisContext::from_parsed`, +2 (`crates/reify-lsp/src/`) | no |
| `reify mcp-server` | `CliToolContext::ensure_engine` (`crates/reify-cli/src/mcp_context.rs`) | no — **being deleted**, #6665 |
| GUI definition preview | `EngineSession::get_def_preview` | no |
| GUI in-process LSP | `LspBridge` → `reify_lsp::bridge::InProcessLsp` | no |
| `reify doc` | `cmd_doc` | n/a — compiles, never evaluates |

`reify mcp-server` is on that list only until #6665 lands: its deletion is ratified
(zero registered consumers; broken since 2026-03 in immediately-visible ways), and
this PRD does no work on it.

Two of those are new findings: the GUI hosts **two** solver-free evaluators inside
a solver-bearing process. Definition previews and editor-surface diagnostics
resolve `auto` differently from the viewport, on the same source, in the same
window. `EngineSession::new` is additionally a `pub` solver-free constructor with
no production caller — a solver-free GUI session is one call away.

Adjacent structural fact worth recording: `Engine::register_solver`, the
`#solver(<name>)` pragma seam, has **zero** production callers, so
`resolve_solver_for_module` always misses and warns. The named-backend feature is
inert in every shipping driver.

### §2.3 — Two divergent implementations of one thing

The resolution problem is built **twice**, by two independently-evolved bodies of
code. `build_solver_problem` / `build_merged_solver_problem`
(`crates/reify-eval/src/engine_eval.rs`) serve the cold and warm-cached paths.
`Engine::edit_param` and `Engine::edit_source`
(`crates/reify-eval/src/engine_edit.rs`) each carry a **byte-identical copy** of a
third, different construction — so the same logic exists in three places and
agrees in none.

Measured divergences between the edit-time population and the cold one:

| | Mechanism |
|---|---|
| edit **gains** | both arms of every guarded group (`EvaluationGraph::from_templates` inserts `group.constraints` *and* `group.else_constraints`; `TopologyTemplate.guarded_groups` is a field separate from `.constraints`, so no guarded constraint ever reaches a cold solve) |
| edit **gains** | purpose-injected constraints |
| edit **gains** | runtime-emitted `forall` per-element constraints |
| edit **loses** | let-mediated constraints (the edit filter has only the direct-read disjunct; cold's `filter_constraints_reading_autos` also tests `CellReadIndex::cells_reaching`) |
| edit **loses** | `dependent_cells` — passes `Vec::new()`, so `fold_dependent_cells` early-returns and coupled `let`s are never recomputed per trial |
| edit **loses** | inherited objectives (reads `self.objectives`, own-scope only; cold uses `governing_objective` + `ContainmentIndex::nearest_container_objective`) |
| edit **loses** | merged cross-scope cluster co-solve |
| edit **loses** | structural-query expansion (`expand_solver_position_expr`) |
| edit **loses** | `#solver` pragma routing |

This is `no-lockstep-duplication` in its most expensive form: nine behavioural
differences, none of them decided, all of them emergent from a copy. It is why
this PRD's remedy is **unification of the implementations**, not nine point-fixes
(Leo, 2026-08-26: *"This divergence smells like duplicated divergent code, and
that concerns me — look for ways to unify the implementations as the primary way
of bringing them into alignment."*).

Note the sharp consequence: `comparison_residual`'s non-numeric fallback is a flat
`1.0` residual, far above `FEASIBILITY_THRESHOLD = 1e-12`. An inactive guard arm's
constraint over an `Undef` cell therefore contributes an irreducible residual, so
a guarded model that solves cold can report `Infeasible` on the first GUI slider
move.

### §2.4 — Determinism is claimed but not held

`whole-model-objective-coupling.md` BT5 states that two `reify eval` runs produce
bit-identical resolved values. They may not. `decompose_into_components_with_reads`
(`crates/reify-constraints/src/decompose.rs`) carries `SubProblem.auto_params` as a
`HashSet<ValueCellId>` and `SolverRegistry::solve_inner` collects it into the
`Vec` that becomes `ResolutionProblem.auto_params` — the solver's **axis order**.
`im::HashMap`/`HashSet` over std `RandomState` iterate differently per *instance*,
not merely per process (verified by execution). Axis order fixes the
start-index↔axis map in `multistart_points`, and start index is the multistart
tie-break key, so two equal-scoring starts in different basins hand the win to
whichever got the lower index. `component_map` is likewise an unsorted
`HashMap`-derived `Vec`, and `solve_inner` early-returns on the *first* failing
component — so which diagnostic surfaces, and which component receives the
objective, are both nondeterministic.

The existing BT5 tests miss this: one uses a spy solver, and
`solve_ranked_multistart_is_deterministic_across_calls` calls `DimensionalSolver`
directly, bypassing `SolverRegistry` entirely.

The engine already knows how to do this correctly. `resolve_order.rs`,
`engine_fixpoint.rs` and `engine_constraints.rs` use explicit `sort_unstable`, a
`BTreeSet<DebugOrd>` ready set described in-source as "the SINGLE source of
determinism … so no HashMap iteration order ever leaks", and `BTreeMap` bucketing
with an original-index weave. `decompose.rs` and `engine_edit.rs` simply do not
follow the house pattern.

Separately: the `#deterministic` module pragma is a **dead flag** —
`apply_deterministic_pragma` (`crates/reify-compiler/src/module_pragmas.rs`) sets
`module.deterministic` and nothing reads it. It is named as the cross-machine
reproducibility escape hatch in `structural-analysis-fea.md`.

---

## §3 — Consumers (G1)

| Consumer | What it consumes | Status |
|---|---|---|
| **printer_v01 dogfood** | The `sub x = Leaf(length: auto)` + `constraint self.x.length == <derived>` idiom, which retires ~7 pinned literals that exist solely because derived cells cannot feed `length:` arguments (#6586's workaround). Requires this PRD's flip **plus** IVF κ #6657 for the realization half. | live, named |
| **spec-conformance program** | Decision 8 — *"Cross-driver agreement (reify check / eval / test / GUI evaluate path / LSP) is itself a conformance surface"*. This PRD is the ruling that closes ★1 and row 8 of its driver-contract matrix, and §10's differential harness is the artifact its suite consumes. | chartered, in scoping |
| **GUI users** | CLI and GUI agreeing on the same file — today the viewport, the definition-preview pane and the GUI's own editor diagnostics disagree with each other in one window. | live |
| **P4 solver legibility** | This committed PRD is P4's stated precondition — P4 surfaces what these drivers report. | chartered |
| **`reify check` as a CI gate** | A green `check` currently means nothing about `auto` resolution. After this PRD it does. | live |

---

## §4 — Contract: the resolution seam (B + H)

G5: blast radius spans `reify-cli`, `reify-eval`, `reify-constraints`, `reify-lsp`,
`reify-mcp` and `gui/src-tauri` (6 ≥ 3); mechanism count ~13 (≥ 8); it touches the
catalogued `ConstraintSolver` seam (`engine-integration-norm.md` §3.5); cross-PRD
consumers ≥ 2. **B + H, unambiguously.**

The contract has one governing idea: **there is exactly one resolution seam, and
it is impossible to route around it.**

### §4.1 — I1: solver-free is unrepresentable

`Engine` acquires a constructor that takes a `ResolutionProfile`, and the
constructors that today yield `solver: None` by omission are removed or made
private. Every construction site in the workspace — production and test — names a
profile. An implementer cannot reproduce the 2026-06 inversion because the
inverted state does not typecheck.

This is the strongest of the three enforcement options considered and was chosen
deliberately over "solver-by-default with a declared opt-out" and "convention plus
a guard test" (Leo, 2026-08-26). The cost is a mass migration of test construction
sites; that cost is the point — it is what makes the property hold.

### §4.2 — I2: wire the registry, never a bare solver

The wiring is exactly:

```rust
engine.with_solver(Box::new(reify_constraints::SolverRegistry::production()))
```

**not** `Box::new(DimensionalSolver)`, and **not** by reusing
`configured_eval_engine` (which also attaches the persistent cache — a staleness
axis a driver must opt into deliberately, per I5).

This is load-bearing, not stylistic. `SolverRegistry::production()` and a bare
`DimensionalSolver` **behave differently on an `auto` that no constraint touches**:
`decompose_into_components_with_reads` drops it, `solve_inner` returns
`Solved { values: {} }`, and the engine's write-back loop leaves the cell `Undef`.
A bare `DimensionalSolver` resolves it to a seed value via
`build_solved_values`'s `initially_feasible && objective.is_none()` early return.
Seven tests currently classified SURVIVES depend on the registry behaviour and
break under a bare solver — wiring the wrong thing roughly doubles the blast
radius.

### §4.3 — I3: one problem builder

Cold `eval`, `eval_cached`, `edit_param`, `edit_source`, and the merged-cluster
dispatchers construct their `ResolutionProblem` through **one** shared builder.
The duplicated block in `engine_edit.rs` is deleted, not aligned. Any legitimate
difference between paths becomes an explicit **parameter** of that builder (which
population source, which budget, which staleness policy) — never a divergent
re-implementation.

Acceptance for this invariant is structural, not behavioural: after the change,
`grep` finds one construction of `ResolutionProblem`'s constraint/auto-param
fields on the engine side. A second one is a regression.

### §4.4 — I4: deterministic by construction at the seam

No `HashMap`/`HashSet`/`PersistentMap` iteration order may reach any field of
`ResolutionProblem`, nor the order of `SubProblem`s in `SolverRegistry`. Ordering
is established by sorting on an `Ord` key (`ValueCellId` already is) or by the
house `BTreeSet<DebugOrd>` pattern. Diagnostics and journal events are sorted
before emission.

### §4.5 — I5: two axes, both typed and surfaced

A driver's `ResolutionProfile` may differ from another's along exactly two axes:

- **Iteration budget** — how many solver iterations it will spend. Exhaustion is
  never silent: it produces a typed cause and a surfaced diagnostic.
- **Staleness** — whether it may serve a previously-computed result (persistent
  compute cache, warm-state pool) instead of recomputing. A served-stale result is
  marked as such.

A profile may **not** decline a stage. This is stronger than the framing this PRD
was chartered under, and follows from Leo's 2026-08-26 overturn of the cost-tier
carve-out: `reify check` gains the FEA compute trampolines it declines today, so
the one blessed example of a stage-decline no longer exists. Collapsing "cost" from
*which stages run* to *how many iterations they get* keeps the invariant total and
— importantly — needs **no amendment to INV-SF-4's doctrine**, which classifies
"a solve that never ran" as an unexpected cause that plain `reify check` must fail
on. Under I5 there is no such thing as a solve that was owed and did not run.

Consequence to be explicit about: `check_fea_violated_constraint_is_not_gated`
(`crates/reify-cli/tests/harness_cli/cli_build_fea.rs`) pins the posture being
overturned and **inverts** under this PRD. `cmd_check`'s posture rustdoc, which
calls that test "an executable contract … changing it requires updating that test
intentionally", is hereby the intentional update.

### §4.6 — I6: an unresolved auto is never a passing verdict

An `auto` that does not reach a designed value produces a typed `Indeterminate`
with an attributable cause (INV-SF-4) and fails plain `reify check`. Three
sub-cases, all probe-verified as silently passing today:

1. **No solver ran.** Eliminated by I1 — the state is unreachable.
2. **Pinned only from one side.** An `auto` whose only pinning constraints are
   one-sided inequalities has no designed value: the Chebyshev-centre objective is
   monotone in that parameter, so its argmax is the edge of `default_bounds_for`'s
   hardcoded domain (LENGTH → `1e-6..10.0`). Probe-verified: `length >= 8mm`
   resolves to **10 m**; `length <= 12mm` resolves to **1 µm**; two-sided bounds
   give the correct centre. A value that is an artifact of a hardcoded box is not
   a resolution — it is an underdetermined design, and it is reported as one with
   a coded diagnostic naming the missing bound.
3. **A contradictory `relate` block.** Probe-verified: `reify check --strict` on a
   model whose `relate` block demands one part be concentric with two holes 15 mm
   apart prints `All constraints satisfied.` and exits 0, while `reify eval` on the
   same file emits a precise conflict diagnostic and exits 1 — because
   `relate_solve::solve_scopes` has exactly one call site,
   `build_with_geometry_output`.

### §4.7 — Determinism tiers

| Tier | Claim | Disposition |
|---|---|---|
| **T1** | Same path, same machine, same process: **bit-identical** resolved values and candidate ordering. | **Closed by this PRD** (I4). Currently claimed by BT5 and not held. |
| **T2** | Cold vs warm-cached vs edit-time, same machine: **identical constraint population and identical governing objective**, and agreement on values **to solver tolerance** (1e-9 rel/abs). | **Closed by this PRD** (I3, plus routing warm through the same `solve_ranked`-vs-`solve` choice as cold). |
| **T3** | Cross-machine reproducibility. | **Explicitly deferred**, not silently ignored. Gated on FEA `ElasticOptions.deterministic` (defaults `false`; execution mode is deliberately excluded from the compute-cache key) and on the `#deterministic` pragma being wired at all. Named in §11. |

T2's value half is **tolerance parity, not bit parity**, and that is a deliberate
design decision, not a weakness. `DimensionalSolver` warm-starts Nelder-Mead from
`current_values`, so a warm re-solve seeds from the previously resolved value while
a cold run seeds from the bounds midpoint; two Nelder-Mead runs from different
simplex origins agree only to optimizer tolerance. The repo has already considered
and rejected re-seeding from the default (it breaks the warm-start integration
tests), and `edit_param_solver_auto_re_resolution_matches_cold` already encodes
tolerance parity as the achievable contract. This PRD adopts that reading and makes
it universal rather than incidental.

---

## §5 — Boundary-test sketch (B + H, two-way)

The §10 φ integration-gate leaf names this table as its observable signal. Rows
face both the producer side (the engine's one seam) and the consumer side (each
driver).

| # | Scenario | Preconditions | Postcondition asserted |
|---|---|---|---|
| B1 | Ctor-arg auto resolves under every driver | `driver_parity_auto_ctl.ri` | `check`, `eval`, `build`, `test`, LSP and GUI all report the constraint `Satisfied`; no driver reports `Indeterminate` |
| B2 | Solved value reaches realized geometry | `driver_parity_auto_ctl.ri` | `reify build` STEP cylinder z-extent = −10 mm; today −5 mm (needs IVF κ #6657 — see §8) |
| B3 | Exact-value verdict survives a float-inexact solve | `constraint self.rod.length == <derived cell>` | solved to ~6e-16 residual and reported `Satisfied`, not `violated` (needs #6653) |
| B4 | One-sided auto is refused, not corner-picked | `driver_parity_one_sided_auto.ri` | every driver emits the coded one-sided diagnostic; `check` exits nonzero; **no** driver reports `length = 10 m` |
| B5 | Two-sided auto resolves to the centre | `driver_parity_two_sided_auto.ri` | resolves to 10 mm on every driver; the one-sided diagnostic does **not** fire |
| B6 | Genuinely infeasible stays infeasible | `== 10mm` ∧ `== 20mm` | every driver reports infeasible with a cause; `check --strict` exits 1; `--strict` stays non-vacuous |
| B7 | Contradictory `relate` fails check | `driver_parity_relate_conflict.ri` | `check` and `check --strict` exit nonzero with the conflict diagnostic `eval` already produces |
| B8 | `@test` over an auto is a real verdict | `@test structure { param x : Length = auto; constraint x > 0 }` | `reify test` reports pass/fail, not `Indeterminate`; dropped `TestResult.diagnostics` are printed |
| B9 | Cold ≡ warm ≡ edit population | a model with a guarded group, a let-mediated constraint and an inherited objective | the three paths build **equal** constraint sets and the **same** governing objective (asserted on the built `ResolutionProblem`, not on values) |
| B10 | Cold ≡ warm values to tolerance | objective-bearing, ≥2 autos, multi-basin | resolved values agree within 1e-9 rel/abs; both paths choose the same solve entry point |
| B11 | Same-machine bit-identity | any ≥2-auto model, via `SolverRegistry::production()` | two `reify eval` runs produce bit-identical values **and** identical candidate ordering; a seeded axis-permutation does not change the result |
| B12 | Budget exhaustion is loud | a model that hits the iteration cap | the cap is reported with a typed cause on every driver, never presented as a converged solve |
| B13 | Staleness is marked | a cached compute result served instead of recomputed | the result is marked stale in the driver's output |
| B14 | GUI internal agreement | any auto-bearing file open in the GUI | viewport, definition-preview pane and in-process LSP report the same resolution |
| B15 | No second problem builder | — | exactly one engine-side construction of `ResolutionProblem`'s constraint/auto fields exists (structural assertion, guards I3 against re-divergence) |

---

## §6 — Resolved design decisions

1. **Enforcement is type-level.** Solver-free is unrepresentable (§4.1). Chosen
   over default-plus-opt-out and convention-plus-guard-test because the failure
   this PRD exists to prevent was caused by the wrong thing being the free default.
2. **`reify mcp-server` is deleted, not brought to parity.** Its deletion is
   already ratified and filed as #6665 (Leo, 2026-08-26). This PRD does **no work**
   on it and files nothing for it; α simply has one fewer construction site to
   migrate once #6665 lands, which is why α is ordered after it (§7.3).
3. **The cost axis is iteration budget, not stage selection** (§4.5). Follows from
   the overturn of the FEA-trampoline carve-out. `reify check` gains the compute
   trampolines.
4. **INV-SF-4's doctrine is applied, not amended.** Because no driver declines a
   stage, "a solve that never ran" retains its literal meaning as an unexpected
   cause. The PRD adds no third cause class and does not widen "expected".
5. **A one-sided-only auto is underdetermined, not resolved** (§4.6 case 2), and
   this PRD owns that refusal rather than deferring it — the flip without it turns
   today's honest silence into a confident 10-metre answer.
6. **Determinism T1 and T2 are both closed here**, and the mechanism is
   **unification of the duplicated implementations** (§2.3, §4.3), not a set of
   point-fixes against the nine observed differences.
7. **T2's value half is tolerance parity.** Warm-start seeding is preserved; bit
   parity across cold/warm is not pursued (§4.7).
8. **This PRD owns driver wiring only.** It does not absorb IVF's realization or
   loud-failure scope, DIC's consumption accounting, or check-diagnostic-truthfulness's
   exit gating (§9).

---

## §7 — Pre-conditions for activating

One hard prerequisite (#6653, covering two distinct failure shapes) and one
ordering prerequisite. #6653 converts this PRD's flip from an improvement into a
regression if absent, because it is what stops correctly-solved models going
**false-red** the moment a solver runs under `check`/`build`.

1. **#6653 — toleranced Scalar equality verdicts.** The engine's post-solve
   equality verdict uses exact `f64` comparison (`crates/reify-expr/src/lib.rs`)
   while the solver accepts at `FEASIBILITY_THRESHOLD = 1e-12`. Every residual in
   the coupled probes (3e-16 … 1.5e-15 m) lands in that dead zone, so `reify eval`
   already prints `AutoProbe.derived = 0.123 m` and
   `AutoProbe.rod.length = 0.12299999999999939 m` and then
   `error: constraint AutoProbe#constraint[0] violated`. Wiring the solver into
   `check`/`build` propagates that whole class. Must land with or before the flip.
   Note the extent: #6653 commits to the engine verdict fold and `DimensionalSolver`
   acceptance only — not `SolveSpaceSolver`, CP-SAT, relate-solve residuals, or
   `RepresentationWithin` tolerance, and it excludes inequality comparisons.

2. **#6653 also covers the mixed equality + inequality collapse.** Independently
   reproduced at HEAD in this session: `constraint L >= 8mm` ∧ `constraint L == 10mm`
   — trivially satisfiable at 10 mm — reports `constraints could not be satisfied
   (max absolute residual: 5.00e-7)` and leaves `L` undef. This is **not** a separate
   prerequisite: #6653's own evidence names this probe and this residual, and its
   acceptance criterion requires all four of its evidence probes to flip to the
   correct verdict. Its work item (2) — replacing `DimensionalSolver`'s flat,
   dimension-blind `FEASIBILITY_THRESHOLD = 1e-12` with the shared relative +
   dimension-aware-absolute-floor policy — is the mechanism that does it (0.5 µm on
   a metre-scale length sits below any sensible engineering floor). Recorded here
   because the flip *exposes* it on every driver: mixing an equality with a
   clearance inequality is the most natural way to write a real engineering
   constraint, so without #6653 the flip converts a silent wrong answer into a
   loud wrong one across the whole corpus.

   Residual concern, tracked separately and **not** blocking: #6653 makes the
   verdict correct by *tolerating* the residual, which leaves the underlying
   systematic bias in place (§8, synthetic-centrality bias).

3. **#6665 — delete `reify mcp-server`.** An ordering prerequisite rather than a
   correctness one. α makes solver-free unrepresentable and must therefore migrate
   every engine construction site in the workspace; `CliToolContext::ensure_engine`
   is one of them and is about to be deleted wholesale. Landing #6665 first removes
   the site instead of migrating a doomed file. Edge wired from α.

---

## §8 — Cross-PRD relationships (G4)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| #6653 verdict tolerance | **depends on** | post-solve Scalar equality verdict fold | #6653 | hard prereq, edge wired |
| synthetic-centrality bias (filed by this session) | **adjacent** | the synthetic max-min-slack objective biases an equality-pinned `auto` by `1/(2·PENALTY_WEIGHT)` whenever an inequality is also present — an *absolute* 5e-7 offset, scale-invariant, so it is 0.005% at 10 mm and 25% at 2 µm | new task | not blocking; #6653 tolerates the symptom, this removes the cause |
| `instantiation-value-flow.md` β #6592, κ #6657 | **consumes** | solved values reaching child realization via the full-cell overlay + effective-cells-differ trigger | IVF | B2's geometry half is κ's; this PRD owns whether the solve runs |
| #6631 (author-surface composite) | **consumes** | its acceptance (a) — build/check resolve autos or refuse loudly — is delivered here; its realization halves ride IVF β | #6631 remains the composite consumer | not absorbed |
| spec-conformance program | **produces for** | cross-driver agreement as a conformance surface (decision 8); this PRD resolves matrix row 8 and ruling question ★1 | this PRD owns the solver row; the program owns the matrix | see note below |
| `check-diagnostic-truthfulness.md` (#5748 β, #5403 γ) | **adjacent** | `reify check` Error-severity exit gating | that PRD | **not absorbed** — see the collision note below |
| `declared-intent-consumption-accounting.md` (#5415–#5421) | **adjacent** | typed `IndeterminateReason`, the check consumption ledger | DIC | not absorbed; §4.6's causes must use DIC's typed vocabulary once #5418 lands |
| IVF γ #6608 | **adjacent** | `reify check` severity-exit convergence | #6608 | not absorbed |
| #6659 registry typed per-constraint refusal | **sibling** | decline-at-recognition vs numeric `NoProgress` | #6659 (in progress) | this PRD's diagnostics must not re-implement it |
| #6646 GUI viewport at-auto pose | **sibling** | pose solve on the GUI load/edit path | #6646 | §4.6 case 3 covers the `check` half only |
| `constraint-solver-completion.md` | **adjacent** | the Chebyshev-centre synthetic objective (its η leaf) | that PRD owns the objective | this PRD owns refusing the one-sided case, not redesigning centrality |
| #6665 delete `reify mcp-server` | **depends on** | removes a construction site α would otherwise migrate | #6665 | ratified; ordering prereq, edge wired |
| `ranked-solve-result.md` | **consumes** | `solve_ranked` / `OptimalityStatus` (I5 back-compat freeze) | that PRD | T2's entry-point convergence relies on its I5 default-method fidelity |
| `docs/prds/v0_6/geometry-algebra-solver-unification.md` (P2) | **parallel** | solver internals, the classifier trap, the lowering table | P2 | landed + decomposed 2026-08-26; disjoint — P1 is wiring, P2 is numerics |
| `docs/prds/v0_6/solver-legibility-telemetry.md` (P4) | **produces for** | this committed PRD is P4's stated precondition — P4 surfaces what these drivers report | P4 | landed 2026-08-26 |
| `docs/prds/v0_6/gui-on-demand-measurement.md` | **adjacent** | GUI kernel measurement for ReprWithin / GD&T / DFM (matrix ★6) | that PRD | not absorbed; §11 |

**Spec-conformance seam note.** The program's two source artifacts —
`docs/notes/driver-contract-matrix-draft.md` and
`docs/notes/cross-driver-divergence-survey-draft.md` — are **untracked drafts** in
the working tree as of authoring. This PRD's row-8 claims are consistent with them
but do not cite them as committed evidence; if they land, the matrix's row 8 and
★1 should be updated to point here.

**Collision to escalate, not resolve here.** Two live tasks own `reify check`'s
`Severity::Error` exit gate with **opposing designs**: #6608 explicitly forbids a
per-code escalation list ("demote/recode rather than exempt"), while #5403 seeds a
`CHECK_ERROR_EXIT_ALLOWLIST` with a burn-down ratchet and carries a G7 waiver for
it. Neither task names the other and there is no edge between them; whichever
lands second inherits a contradiction. This PRD depends on neither and must not
pick a winner — but §4.6's "fails plain check" requirement lands on top of
whichever wins, so the collision is worth resolving before either dispatches.

---

## §9 — Sketch of approach

Phase 1 builds the seam; phase 2 is the vertical slice that proves it; phases 3–5
extend it to the remaining drivers and close determinism; phase 6 is docs-truth;
phase 7 closes the PRD.

The ordering is deliberate: the seam comes **before** the flip. Flipping first and
unifying later would mean scaffolding inside the functions that the unification
rewrites wholesale, and would leave the invariant as a convention for however long
phase 1 took to arrive — which is exactly the state that permitted the 2026-06
inversion.

---

## §10 — Decomposition plan (decomposed 2026-08-26; task IDs stamped)

Greek labels carry their real task IDs. Phase order is dependency order; every
edge below is a real `add_dependency` edge, not prose ordering.

**G7 walk (decompose, against `docs/legibility/design-invariants.md`).** All 15
leaves walked against INV-SF-1..7. One finding, **resolved by refinement rather
than waived**: ε (#6692) originally specified a runtime typed `Indeterminate` for a
one-sided `auto`, but INV-SF-4 reserves `Indeterminate` for run-dependent causes and
states that a constraint indeterminate in *every* possible run is a **compile
error**. One-sidedness is a property of the constraint set, not of any runtime
value, so ε now carries a two-arm requirement — a static compile-time diagnostic
where the pinning set is statically determinable (the common case and the preferred
arm), falling back to a typed runtime Indeterminate only where the population is
genuinely not static (guarded, purpose-injected or `forall`-emitted constraints, per
β #6690). Stamped in ε's `details`. **No G7 waivers were recorded for this batch.**

**Phase 1 — the seam.**

- **α (#6689) — `ResolutionProfile` + profile-taking `Engine` constructor; solver-free
  becomes unrepresentable.** Modules: `reify-eval`, plus every construction site
  workspace-wide. Removes/privatises the solver-free-by-omission constructors and
  migrates all callers including tests. *Intermediate* — unlocks β, δ, and every
  phase-3 driver leaf. Signal: unlocks δ; the workspace compiles with no
  construction path that yields `solver: None` by omission. `grammar_confirmed: true`.
- **β (#6690) — One resolution-problem builder; delete the duplicated `engine_edit.rs`
  block.** Modules: `reify-eval` (`engine_eval.rs`, `engine_edit.rs`). Cold,
  cached and both edit entries construct `ResolutionProblem` through one builder;
  population source / budget / staleness become parameters. *Intermediate* —
  unlocks κ and φ; delivers T2's population half. Signal: unlocks κ; B9 and B15.
- **γ (#6691) — Deterministic ordering at the seam (T1).** Modules: `reify-constraints`
  (`decompose.rs`, `registry.rs`), `reify-eval`. Sort every `Hash*`→`Vec` reaching
  `ResolutionProblem` and the `SubProblem` list; sort diagnostics before emission;
  add a detector for the pattern. *Leaf.* Signal: B11 — two `reify eval` runs of a
  ≥2-auto model through `SolverRegistry::production()` produce bit-identical values
  and candidate ordering, and a seeded axis permutation does not change the result.

**Phase 2 — the flip (integration gate).**

- **ε (#6692) — One-sided-only `auto` is underdetermined: typed, coded, loud.** Modules:
  `reify-constraints` (`solver.rs` bound analysis), `reify-core` (diagnostic code),
  `reify-eval`. *Leaf.* Signal: B4 + B5 — `length >= 8mm` alone emits the coded
  diagnostic on every driver and `check` exits nonzero; `>= 8mm` with `<= 12mm`
  resolves to 10 mm and the diagnostic does not fire. Depends on α.
- **δ (#6693) — Flip `cmd_build` and `cmd_check` to the full profile; retire the posture
  artifacts; re-author the pinned tests.** Modules: `reify-cli` (`main.rs`),
  `reify-cli/tests/harness_cli/*`, `crates/reify-cli/tests/fixtures/*.ri`,
  `crates/reify-eval` (the `with_registered_kernel` rustdoc cross-cite). Wires
  `SolverRegistry::production()` per I2 **and** the compute trampolines into
  `cmd_check`; deletes the `cmd_build` posture comment, the matching
  `configured_eval_engine` rustdoc note, and `cmd_check`'s posture rustdoc;
  inverts `check_fea_violated_constraint_is_not_gated`; re-authors the
  assert-text-breaking tests; replaces the `bracket_*indeterminate.ri` fixtures
  with genuinely-unresolvable ones. **Leaf — names the §5 boundary-test sketch
  (B1, B3, B6) as its signal.** Depends on α, ε; **and on #6653** (§7).
  `grammar_confirmed: true`.

  *Fixture-authoring rule (load-bearing, from the blast-radius measurement):*
  replacement `--strict` fixtures must contain **zero `auto` params**. An
  undef-operand constraint that also reads an `auto` poisons the solve —
  `comparison_residual`'s `_ => 1.0` arm makes the component infeasible and emits
  `Severity::Error`, flipping `build` to exit 1. With no autos,
  `filter_constraints_reading_autos` removes the constraint from the problem
  entirely and `build`'s exit-0 contract is preserved. Each new fixture carries a
  comment forbidding the later addition of an `auto`.

**Phase 3 — the remaining drivers.**

- **ζ (#6694) — `reify test` runs the full resolution and reports real verdicts.**
  Modules: `reify-eval` (`test_runner.rs`), `reify-cli`. Replaces
  `build_test_engine`'s solver-free construction; prints the `TestResult`
  diagnostics the runner currently drops; retires the stale dev-dependency
  rationale in its doc comment (`reify-constraints` has been a normal dependency of
  `reify-eval` since task 4386). Inverts
  `run_tests_with_auto_param_returns_indeterminate`. *Leaf.* Signal: B8. Depends on α, δ.
- **η (#6695) — LSP resolves, with a bounded budget and marked staleness.** Modules:
  `reify-lsp` (all four construction sites, including `AnalysisContext::from_parsed`
  which backs hover/completion/goto-def). *Leaf.* Signal: an `auto`-bearing file
  open in an editor shows the resolved value on hover and the same constraint
  verdict the CLI reports; budget exhaustion surfaces as a diagnostic rather than
  a silent indeterminate. Depends on α, δ.
- **ι (#6696) — The GUI's two internal solver-free evaluators.** Modules: `gui/src-tauri`
  (`engine.rs` `get_def_preview`, `lsp_bridge.rs`), and `EngineSession::new`.
  *Leaf.* Signal: B14. Depends on α, η.
- **ρ (#6698) — `relate` resolution reaches `check`.** Modules: `reify-eval`
  (`relate_solve.rs` call sites, `engine_constraints.rs`). A contradictory `relate`
  block is no longer invisible to `check`; where the pose solve genuinely cannot run
  on a given driver, the relate block is reported unresolved with a typed cause
  rather than folded into `All constraints satisfied.` *Leaf.* Signal: B7. Depends
  on α, δ. **Seam:** the GUI-viewport half is #6646's; the zero-auto static
  verification arm is DIC α #5415's; this leaf owns the `check` half only.

**Phase 4 — cold ≡ warm ≡ edit (T2's value half).**

- **κ (#6699) — Warm and edit paths choose the same solve entry point as cold.** Modules:
  `reify-eval` (`engine_eval.rs`, `engine_edit.rs`). Cold calls
  `solve_ranked_with_dispatch` when an objective is present; warm calls
  `solve_with_dispatch` unconditionally. Routes both through the same choice.
  *Leaf.* Signal: B10 — an objective-bearing, ≥2-auto, multi-basin model resolves
  to the same optimum cold and warm, and `W_SOLVER_OPTIMALITY_UNPROVEN` (cold-only
  today) surfaces on both. Depends on β.

**Phase 5 — the differential harness (second integration gate).**

- **φ (#6700) — Committed cross-driver agreement harness.** Modules: a test target plus
  `tests/prd-gate/fixtures/`. Drives one fixture corpus through `check`, `eval`,
  `build`, `test`, the LSP and the GUI session and asserts equal resolution.
  **Leaf — names the whole §5 sketch as its signal.** Depends on δ, ζ, η, ι, ρ,
  κ, γ. This is the artifact the spec-conformance suite consumes (§3).

**Phase 6 — docs-truth.**

- **χ (#6701) — Invariant text + registry row.** Modules: `docs/legibility/design-invariants.md`,
  `docs/invariants.md`. Appends a new **driver-parity family (INV-DR-\*)** with a
  family intro paragraph dating it and naming this session, each member in the
  house four-part shape (Rule / Checkable design question(s) / Evidence / House
  pattern); adds the paired registry row per INV-META-1, citing `CacheLeg::Skip`
  (INV-EVAL-1) as the "a skip is an explicit value, enforced by type" house
  pattern. Cites INV-SF-3 and INV-SF-4 by slug rather than restating them.
  *Leaf.* Signal: the committed invariant text. Depends on δ, γ, κ. **Lands with
  the fix, not before** (doc-truth).
- **ψ (#6702) — Doc chunks, CLI help, README, getting-started, spec.** Modules:
  `crates/reify-mcp/src/tools/chunks/`, `crates/reify-cli/src/main.rs`,
  `README.md`, `docs/getting-started.md`, `docs/reify-language-spec.md`. Adds the
  missing **"letting the solver choose a dimension"** chunk — today no chunk names
  a single CLI command, none documents `--strict`, and none carries the
  `sub x = Leaf(length: auto)` binding site at all; driver-qualifies the
  "solver decides" claims in `parameters.md`/`syntax.md`/`types.md`/`structures.md`;
  fixes `constraints.md`'s "while the build reports success" (false today); fixes
  `README.md`'s `check <file>  Parse, type-check, solve constraints` (flatly false
  today) and its 5-of-8 subcommand roster; adds `--strict`/`--purpose`/`--cfg` to
  `print_usage`; reconciles spec §10.2's three-mode hierarchy, the `UndefCause`
  enumeration, and the counted "three tooling surfaces" claim. Updates the
  `TOPICS` array, `get_chunk` and the pinned `available_topics_returns_17_entries`
  test together. *Leaf.* Signal: **discoverability acceptance** — an author who
  knows the goal ("let the solver pick this dimension") but not the feature name
  reaches the mechanism from the chunks or the corpus index. Depends on δ.
- **ω (#6703) — Exemplar corpus.** Modules: `examples/best_practices/`, its `INDEX.md`,
  `.claude/skills/reify-design/SKILL.md`. Graduates `examples/auto_binding_sites.ri`
  (which today has no `INDEX.md` row and is not in `best_practices/`) into the
  corpus as the solver-binding-site exemplar; fixes the `INDEX.md` rows asserting
  "`reify check` reports these INDETERMINATE, which is expected" and "`reify check`
  will not tell you when it doesn't"; adds the one-line `SKILL.md` index entry
  pointing at it. File and row land in one commit
  (`best_practices_index_matches_corpus_directory`). *Leaf.* Signal: the committed
  exemplar runs green under `check`, `eval` and `build` with the same resolution.
  Depends on δ.

**Phase 7 — close.**

- **Ω (#6704) — PRD close.** Docs-only. Backfills real task IDs into §10, sets the terminal
  `Status:` marker, adds the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED
  map, and applies the matching header to the capability manifest. *Leaf.* Signal:
  the committed header. Depends on every other leaf.

**DAG (all real edges):**
`#6665 → α`; `α → {β, γ, ε}`; `{α, ε, #6653} → δ`; `β → κ`;
`{α, δ} → {ζ, η, ρ}`; `δ → {ψ, ω}`; `{α, η} → ι`; `{δ, γ, κ} → χ`;
`{γ, δ, ζ, η, ι, ρ, κ} → φ`; every leaf → `Ω`.

Two out-of-batch edges are load-bearing and real: **#6653** (toleranced equality
verdicts) gates δ, and **#6665** (delete `reify mcp-server`) gates α. **#6688**
(synthetic-centrality bias), filed by this session, is deliberately *not* an edge —
it is a quality follow-up that #6653 renders non-blocking.

---

## §11 — Out of scope

- **Cross-machine determinism (T3).** Deferred explicitly. It requires FEA's
  `ElasticOptions.deterministic` to stop defaulting to `false`, the compute-cache
  key to stop excluding execution mode, and the `#deterministic` module pragma to
  be read by anything at all (it is a dead flag today). Named, not silently ignored.
- **Redesigning the Chebyshev-centre objective.** This PRD refuses the one-sided
  case; the objective itself belongs to `constraint-solver-completion.md`'s η leaf.
- **`reify check`'s Error-severity exit gate.** Owned by #5403 / #6608 (§8).
- **The kernel and cache reach of `report` / `explain` / `build`** — matrix rows
  10–14, ★2. Solver row only here.
- **Constraint-verdict contract per driver** (matrix ★3) — `eval`'s
  geometry-dependent gating accident, `test`'s exit posture, `report`'s
  violation-blindness. ζ prints `test`'s dropped diagnostics; the exit contract is
  the conformance program's ruling.
- **Purpose surfaces beyond `check`** (matrix ★4) and **module-header enforcement
  in GUI/LSP** (★5).
- **Solver numerics** — multistart quality, `minimize` making no progress from its
  seed (probe-observed: `minimize length` with `length >= 8mm` returns exactly
  `8mm × 1.1`, the seed nudge), CP-SAT wiring (#5469), interval seeding (#5655).
  P2 and the solver-internals batch.
- **`reify mcp-server`, entirely.** Its deletion is ratified and owned by #6665.
  This PRD does no work on it — not the solver wiring, not the no-prelude bootstrap
  `compile()`, not the hardcoded `"unknown"` constraint status, not the stripped
  diagnostic codes. It appears here only as an ordering prerequisite for α (§7.3).
- **`reify doc`** — it compiles and never evaluates, so it is not a resolution
  driver.
- **Solved-value realization at the child boundary** — IVF β #6592 / κ #6657.

---

## §12 — G6 premise-validity notes

Every numeric and capability claim in §1, §5 and §7 was measured at HEAD
`2128c3692c` with `target/debug/reify`, verified fresh: 40 commits since the build,
**zero** touching `crates/reify-{cli,eval,constraints,compiler,ir,expr,core,runtime,stdlib}`.
100 probe invocations plus 20 authored controls; no environmental failures.

- **−5 mm vs −10 mm (B2)** is measured, not assumed: `out/auto_ctl.step` carries
  `CARTESIAN_POINT('',(0.,0.,-5.))` while `reify eval` on the same file prints
  `AutoCtl.rod.length = 0.01 m`. A control fixture with a `7mm` default exports
  z = −7, confirming the STEP tracks the default exactly rather than a failed solve.
- **10 m and 1 µm (§4.6 case 2)** are `default_bounds_for(LENGTH) = (1e-6, 10.0)`,
  read from source and reproduced independently in this session. The two-sided
  control resolves to the analytic centre (10 mm), so the diagnostic in ε has a
  discriminating negative case and cannot be vacuous.
- **The eq+ineq residual `5.00e-7` (§7.2, owned by #6653)** is reproduced
  independently in this session and matches `2·PENALTY_WEIGHT·d = 1` with `PENALTY_WEIGHT = 1e6` to all
  printed digits; it is scale-, direction-, order- and strictness-invariant across
  six controls. The *derivation* is inferred; the *observation* is not.
- **B3's ~6e-16 residual** is measured (`0.12299999999999939` against a target of
  `0.123`), as is the `error: constraint … violated` that today accompanies it.
- **B7 is measured**, not reasoned from code: an authored contradictory-relate
  control returns exit 0 and `All constraints satisfied.` under `check --strict`
  and exit 1 with a precise conflict diagnostic under `eval`.
- **B11's premise** — that std hash iteration order varies per *instance* within a
  single process and thread — was verified by executing a standalone program, not
  inferred from documentation.
- **All five committed fixtures were executed at HEAD** and each reproduced its
  documented baseline exactly, including `driver_parity_relate_conflict.ri`, where
  `check --strict` was observed exiting **0** with `All constraints satisfied.` on a
  model `eval` rejects — the vacuity ρ closes.
- **Negative-assertion discipline.** B4, B6 and B7 each assert a rejection. B4 and
  B7's rejections are **absent today** and observed to be absent (the substrate
  check exits 0 with no diagnostic), so both are queued as producing leaves (ε, ρ)
  rather than logged as motivation. B6's rejection **is** present today and was
  observed firing on `infeasible.ri`, so it is a preservation assertion.
- **No guessed numeric bound.** The only tolerance asserted is T2's 1e-9 rel/abs,
  which is not a guess — it is `SOLVER_AUTO_PARITY_ABS_TOL`/`_REL_TOL`, already in
  the tree and already justified in `edit_param_solver_auto_re_resolution_matches_cold`'s
  own doc comment. Everything else is an exact value with an analytic basis (the
  10 mm centre of `[8, 12]`) or a byte/exit-code assertion.
- **DAG direction.** B2's geometry half is owned by IVF κ #6657, which is **not**
  downstream of this PRD — it is a sibling with its own upstream (#6592, #6608).
  B2 is therefore stated in §5 as this PRD's *consumer-facing* outcome and is
  **not** the signal of any leaf here; δ's signal is B1/B3/B6, all of which this
  PRD's own dependency set produces.

---

## §13 — Open questions (tactical, deferred to implementation)

1. **Where `ResolutionProfile` lives** — `reify-ir` (alongside `ResolutionProblem`)
   vs `reify-eval` (alongside `Engine`). Suggested: `reify-ir`, so `reify-lsp` and
   `gui/src-tauri` can name a profile without depending on the engine's internals.
   Decide during α.
2. **How far the α migration privatises.** Removing the solver-free constructor
   outright vs `#[doc(hidden)]` + a `ResolutionProfile::none()` that test callers
   name explicitly. The latter is a far smaller diff and still makes the state
   nameable-only-on-purpose. Suggested: `ResolutionProfile::none()`, named at every
   test site. Decide during α.
3. **The LSP's iteration budget value.** Suggested: start at the warm
   `500·(n+1)` scaling already used for feasible-with-objective solves, and measure.
   Decide during η.
4. **Diagnostic code spelling** for the one-sided refusal (ε) and for budget
   exhaustion / staleness markers (I5). Must be coordinated with DIC #5418's typed
   `IndeterminateReason` and with #6659's decline-at-recognition vocabulary rather
   than minting a parallel taxonomy. Decide during ε, in consultation with those
   tasks' owners.
5. **Whether φ drives the GUI in-process or through the debug MCP listener.**
   Suggested: in-process `EngineSession`, with the debug listener reserved for the
   viewport-specific rows. Decide during φ.
6. **Whether γ's detector is a `reify-audit` pattern or a Rust test.** Suggested:
   a Rust test in `reify-constraints` first (cheapest, closest to the seam); an
   audit pattern only if the shape recurs outside the solver crates. Decide during γ.
