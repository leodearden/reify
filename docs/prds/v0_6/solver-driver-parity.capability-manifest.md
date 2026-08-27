# Capability manifest — Solver driver parity (solve everywhere)

PRD: `docs/prds/v0_6/solver-driver-parity.md`. Machine-readable twin:
`docs/prds/v0_6/solver-driver-parity.capability-manifest.yaml`.

Mechanizes G3 + G6 per leaf: every capability a leaf's signal asserts is bound to
evidence, so the substrate check is paid once here rather than once per task at
dispatch. Any FAIL binding blocks the batch.

**Evidence base.** All observations were made at main
`2128c3692cbb88f59b6e9edfd25ee801513423bb` (2026-08-26) with `target/debug/reify`,
verified fresh: 40 commits since the binary was built, **zero** touching
`crates/reify-{cli,eval,constraints,compiler,ir,expr,core,runtime,stdlib}`. 100 probe
invocations plus 20 authored controls, zero environmental failures. The five
committed fixtures under `tests/prd-gate/fixtures/driver_parity_*.ri` each parse with
0 ERROR nodes and reproduce the baseline recorded in their own headers.

**Verdict summary: 15 leaves, 44 bindings, 0 FAIL.**

---

## Negative-assertion sentinel — the three inverted checks

Three leaves assert a rejection. Per the sentinel rule, each was authored and run,
and the asserted diagnostic was **observed to be ABSENT** — which confirms the
rejection capability does not exist and correctly makes each a *producing* leaf
rather than a preservation assertion. Logging the contradicting silent-accept as
test motivation instead of binding it is the failure this rule exists to prevent.

| Leaf | Asserted rejection | Observed today | Disposition |
|---|---|---|---|
| ε #6692 | a one-sided-only `auto` is refused | `reify eval` returns `10 m`, exit 0, **no diagnostic** | rejection ABSENT → ε produces it |
| ρ #6698 | a contradictory `relate` fails `check` | `check` **and** `check --strict` exit 0, `All constraints satisfied.` | rejection ABSENT → ρ produces it |
| δ #6693 | a genuinely infeasible model still fails | `eval` exits 1, `max absolute residual: 5.00e-3` | rejection **PRESENT** → δ preserves it |

δ's row is deliberately the opposite shape: it is a preservation assertion, so the
manifest binds that the mechanism already fires rather than queueing a producer.

---

## Per-leaf bindings

### α #6689 — ResolutionProfile; solver-free unrepresentable

| Capability | Binding | Verdict |
|---|---|---|
| `solver-registry-production-exists` | capability→producer, **wired on main** — `SolverRegistry::production()` is a real constructor and is the wiring used by both existing production sites | PASS |
| `two-production-wiring-sites-only` | observed — exhaustive grep finds exactly two non-test `SolverRegistry::{production,new,with_solvers}` / `.with_solver` sites: `configured_eval_engine` and `EngineSession::with_registered_kernel`. Every other is under `#[cfg(test)]` or `tests/` | PASS |
| `solver-free-is-by-omission` | producer-self — `Engine::with_prelude` initialises `solver: None`; only `with_solver`/`register_solver` set it. This is the state α makes unnameable-by-accident | PASS |
| `mcp-server-site-removed-upstream` | **DAG-direction** — `CliToolContext::ensure_engine` is a site α would otherwise migrate; #6665 (delete `reify mcp-server`) is **upstream** of α by a real edge, so the site is gone before α runs. Not downstream | PASS |

### β #6690 — one resolution-problem builder

| Capability | Binding | Verdict |
|---|---|---|
| `duplicated-edit-block-exists` | observed — `edit_param` and `edit_source` carry a byte-identical construction distinct from `build_solver_problem` | PASS |
| `cells-reaching-index-exists` | capability→producer, **wired on main** — `CellReadIndex::cells_reaching` is what cold's `filter_constraints_reading_autos` uses and the edit filter lacks; β adopts it rather than inventing it | PASS |
| `guarded-groups-are-a-separate-field` | observed — `TopologyTemplate.guarded_groups` is a field separate from `.constraints`, and `EvaluationGraph::from_templates` inserts both `group.constraints` and `group.else_constraints`. This is the mechanism behind the population divergence | PASS |

### γ #6691 — deterministic ordering (T1)

| Capability | Binding | Verdict |
|---|---|---|
| `hash-order-reaches-solver` | observed — `SubProblem.auto_params` is a `HashSet<ValueCellId>` collected into the `Vec` that becomes `ResolutionProblem.auto_params`; `component_map` is likewise unsorted | PASS |
| `valuecellid-is-ord` | capability→producer, **wired on main** — `ValueCellId` already implements `Ord`, so the sort key exists and γ need not add one | PASS |
| `house-sort-pattern-exists` | capability→producer, **wired on main** — `resolve_order.rs`, `engine_fixpoint.rs` (`BTreeSet<DebugOrd>`) and `engine_constraints.rs` already implement the discipline γ ports to the two sites that lack it | PASS |
| `per-instance-hash-variance` | observed **by execution**, not inferred from documentation — three `HashSet`s built from the same six keys in one process, one thread, iterate in three different orders | PASS |

### ε #6692 — one-sided auto is underdetermined

| Capability | Binding | Verdict |
|---|---|---|
| `one-sided-corner-pick` | **rejection-check, OBSERVED ABSENT** — `reify eval tests/prd-gate/fixtures/driver_parity_one_sided_auto.ri` → `10 m`, exit 0, no diagnostic. Reproduced independently twice | PASS |
| `two-sided-still-resolves` | observed — `driver_parity_two_sided_auto.ri` → `0.01 m`, the **analytic** centre of `[8, 12]`. The discriminating negative case; the asserted value has a closed-form basis, not a tuned guess | PASS |
| `default-bounds-are-hardcoded` | observed in source — `default_bounds_for` returns `(1e-6, 10.0)` for LENGTH and `build_auto_param_list` always passes `bounds: None`, so constraint-derived bounds never narrow the box. This is *why* the argmax is 10 m | PASS |
| `diagnostic-code-registry-exists` | capability→producer, **wired on main** — `crates/reify-core/src/diagnostics.rs` is the live code registry; ε adds an arm, it does not create the mechanism | PASS |
| `grammar-fixture` | grammar-fixture — both fixtures parse with **0 ERROR** nodes under `tree-sitter parse --quiet` | PASS |

### δ #6693 — the flip

| Capability | Binding | Verdict |
|---|---|---|
| `verdict-tolerance` | **DAG-direction, the load-bearing one** — the exact-`f64` verdict fold vs the solver's 1e-12 acceptance is owned by **#6653**, which is **upstream** of δ by a real edge. Without it the flip converts silent-wrong into false-red across the corpus | PASS |
| `verdict-tolerance-extent` | **producer-extent** — #6653 commits to the engine verdict fold *and* `DimensionalSolver` acceptance, and its acceptance criterion names all four evidence probes including the eq+ineq one. It does **not** cover `SolveSpaceSolver`, CP-SAT, relate-solve residuals or `RepresentationWithin`, and excludes inequality comparisons. δ's signal (B1/B3/B6) stays inside the covered extent | PASS |
| `compute-trampoline-registration` | capability→producer, **wired on main** — `register_compute_trampolines` exists and `cmd_build` already calls it directly; δ extends the call to `cmd_check` | PASS |
| `fea-posture-pin-inverts` | observed — `check_fea_violated_constraint_is_not_gated` exists and pins the posture Leo overturned on 2026-08-26. δ inverting it is the intentional update `cmd_check`'s own rustdoc demands | PASS |
| `bracket-fixtures-are-destroyed` | observed — running the solver-wired driver on each of the four (plus the byte-identical fifth) is a *direct measurement* of post-flip behaviour: `bracket_indeterminate` → `10 m`, `bracket_all_indeterminate` → both autos resolve, `bracket_violated_with_indeterminate` loses its indeterminate half | PASS |
| `infeasible-still-refused` | **rejection-check, OBSERVED PRESENT** — `== 10mm` ∧ `== 20mm` → `max absolute residual: 5.00e-3`, nine orders above the threshold, unresolvable under any wiring. Preservation assertion, not a producer | PASS |
| `replacement-fixture-rule` | producer-self, **authoring constraint** — replacement `--strict` fixtures must carry zero `auto` params: `comparison_residual`'s `_ => 1.0` arm makes an undef-operand constraint that also reads an auto infeasible and `Severity::Error`, flipping `build` to exit 1 | PASS |

### ζ #6694 — `reify test`

| Capability | Binding | Verdict |
|---|---|---|
| `testresult-diagnostics-field-exists` | **field-population** — `TestResult.diagnostics: Vec<Diagnostic>` is a real, populated field ("Diagnostics emitted by constraint checking during the test run"); the runner's loop simply never reads it. The capability is *present and dropped*, not absent — ζ prints what already exists | PASS |
| `reify-constraints-is-a-normal-dep` | capability→producer, **wired on main** — `crates/reify-eval/Cargo.toml` carries `reify-constraints.workspace = true` under `[dependencies]` (promoted by task 4386). `build_test_engine`'s "dev-only dependency" rationale is **stale**; the boundary objection is false | PASS |
| `pinning-test-exists` | observed — `run_tests_with_auto_param_returns_indeterminate` pins the current posture and is the contract ζ intentionally inverts | PASS |

### η #6695 — LSP

| Capability | Binding | Verdict |
|---|---|---|
| `four-solver-free-lsp-sites` | observed — `EvalState::new`, the cold-start rebuild inside `compute_diagnostics_with_state`, standalone `compute_diagnostics`, and `AnalysisContext::from_parsed`. The fourth backs hover/completion/goto-def; missing it would leave the LSP disagreeing with itself | PASS |
| `iteration-budget-mechanism-exists` | capability→producer, **wired on main** — the `500*(n+1)` feasible-with-objective scaling already exists, so η tunes a budget rather than inventing budgeting | PASS |
| `no-wallclock-budget-to-preserve` | observed — there is no `Executor` timeout and no `Duration` deadline anywhere in `reify-constraints`; every budget is an iteration or node count. η must not introduce the first wall-clock bound, since that would make solves load-dependent | PASS |

### ι #6696 — the GUI's two internal solver-free evaluators

| Capability | Binding | Verdict |
|---|---|---|
| `get-def-preview-is-solver-free` | observed — `EngineSession::get_def_preview` builds `Engine::new(SimpleConstraintChecker, None)` per preview and `check()`s it, inside a solver-bearing session | PASS |
| `in-process-lsp-is-solver-free` | observed — `LspBridge` → `reify_lsp::bridge::InProcessLsp` → the real `ReifyLanguageServer`, all solver-free | PASS |
| `engine-session-new-is-a-latent-hole` | observed — `EngineSession::new` is `pub` and `from_engine` deliberately does not install the solver; only `with_registered_kernel` does | PASS |
| `lsp-work-is-upstream` | **DAG-direction** — ι wires the GUI's LSP host; the LSP resolution itself is η #6695, which is **upstream** of ι by a real edge. ι does not re-implement it | PASS |

### ρ #6698 — relate resolution reaches `check`

| Capability | Binding | Verdict |
|---|---|---|
| `relate-vacuity-under-check` | **rejection-check, OBSERVED ABSENT** — `check` and `check --strict` on `driver_parity_relate_conflict.ri` both exit 0 with `All constraints satisfied.`, printing zero per-constraint lines for the relate block | PASS |
| `eval-conflict-diagnostic-exists` | capability→producer, **wired on main and observed firing** — `reify eval` on the same fixture exits 1 naming the relation, both operands, the required separation and the primary conflict. ρ routes `check` to this existing message rather than authoring a second one | PASS |
| `solve-scopes-single-call-site` | observed — `relate_solve::solve_scopes` is reached only from `build_with_geometry_output`, which is precisely why `check` never sees it | PASS |
| `seam-not-absorbed` | **DAG-direction / scope** — the GUI-viewport half is #6646, the zero-auto relate arm is DIC α #5415, auto-ful relate diagnostics are #4388. None is downstream of ρ; ρ owns the `check` half only | PASS |

### κ #6699 — cold/warm solve entry-point convergence

| Capability | Binding | Verdict |
|---|---|---|
| `entry-point-split-exists` | observed in source — cold calls `solve_ranked_with_dispatch` iff `objective.is_some()`; warm calls `solve_with_dispatch` unconditionally | PASS |
| `divergence-is-self-documented` | observed — `SolverRegistry::solve_ranked`'s own doc states candidate 0 "is never worse than `solve()`'s, but not guaranteed byte-identical", and `solve_ranked_impl`'s comment states the two "can disagree on Solved-vs-Infeasible for the SAME problem" | PASS |
| `solve-ranked-is-back-compat` | capability→producer, **wired on main** — `ranked-solve-result.md` I5 guarantees the default `solve_ranked` lifts `solve()` losslessly, so routing warm through it cannot change below-gate behaviour | PASS |
| `tolerance-basis-is-in-tree` | **numeric floor — not a guessed bound** — the asserted 1e-9 rel/abs is `SOLVER_AUTO_PARITY_ABS_TOL`/`_REL_TOL`, existing constants whose own doc justifies tolerance-parity as "the achievable, load-bearing contract". **Caveat for the implementer:** they currently live in `crates/reify-eval/tests/common/differential.rs` (test-support), so κ must promote or re-home them to state the contract in production terms | PASS |

### φ #6700 — differential harness

| Capability | Binding | Verdict |
|---|---|---|
| `every-driver-is-upstream` | **DAG-direction** — φ asserts agreement across drivers it does not itself fix. All seven producing leaves (γ, δ, ζ, η, ι, ρ, κ) are **upstream** of φ by real edges. No capability φ asserts is owned by a task that depends on φ | PASS |
| `fixture-corpus-committed` | grammar-fixture — all five `driver_parity_*.ri` fixtures are committed with this PRD and parse with 0 ERROR nodes | PASS |
| `gui-debug-listener-exists` | capability→producer, **wired on main** — the GUI debug MCP listener is an observation surface over the same `EngineSession` pipeline, so φ can drive the GUI without a fourth pipeline | PASS |
| `registration-obligations-same-diff` | producer-self, **gate obligation** — φ adds gate-resident tests, so `_RUST_COUPLED_RI_FIXTURES`, `run-all-classification.manifest`, the wallclock-bounds registration and `.config/nextest.toml` entries all land in φ's own diff. Ordering any of them after φ is the esc-4914-162 failure | PASS |

### χ #6701 — invariant text

| Capability | Binding | Verdict |
|---|---|---|
| `invariant-file-and-family-convention` | capability→producer, **wired on main** — `docs/legibility/design-invariants.md` exists, its preamble explicitly contemplates appended families, and INV-AD-* is the worked precedent for adding one | PASS |
| `registry-row-required` | capability→producer, **wired on main** — INV-META-1 in `docs/invariants.md` requires the paired row; `INV-EVAL-1` (`CacheLeg::Skip`, "skipped legs are explicit values, never omissions") is the house pattern to cite | PASS |
| `doctrine-needs-no-amendment` | producer-self, **design claim** — because the ruled cost axis bounds iterations and never declines a stage, "a solve that never ran" keeps its literal INV-SF-4 meaning. χ cites slugs and must not widen "expected" | PASS |
| `stale-evidence-lines-identified` | observed — INV-SF-1, INV-SF-2 and INV-SF-4 each carry an Evidence line the flip falsifies; χ repairs them in place | PASS |

### ψ #6702 — docs-truth

| Capability | Binding | Verdict |
|---|---|---|
| `readme-line-is-false` | observed — `README.md` line 79 reads `reify check <file>  Parse, type-check, solve constraints` while `cmd_check` installs no solver. The one flatly false user-facing line | PASS |
| `zero-chunk-cli-coverage` | observed — grep for `reify check|reify build|reify eval|--strict|CLI` across all 17 chunks returns **nothing**; "solver" appears 3 times in 1211 lines | PASS |
| `chunk-registry-and-pinned-count` | capability→producer, **wired on main** — `TOPICS`, `get_chunk`, `available_topics()` and the pinned `available_topics_returns_17_entries` all exist and must be updated together | PASS |
| `no-automated-gate-covers-this` | observed — `reify-audit --pattern PDOCCOVER` is a registry↔chunk *name*-drift detector and structurally cannot see a false claim about which command solves. ψ is the enforcement | PASS |

### ω #6703 — exemplar corpus

| Capability | Binding | Verdict |
|---|---|---|
| `source-exemplar-exists` | capability→producer, **wired on main** — `examples/auto_binding_sites.ri` exists and already carries all four binding-site spellings including `sub bolt = Bolt(length: auto)`. ω graduates it; it does not author it | PASS |
| `index-guard-exists` | capability→producer, **wired on main** — `best_practices_index_matches_corpus_directory` enforces bidirectional file↔row correspondence, so file and row must land in one commit | PASS |
| `false-index-rows-identified` | observed — the `clearance_oracle.ri`, `symmetry_mirror.ri` and `discrete_choice.ri` rows each assert a check-vs-eval split this PRD removes | PASS |
| `corpus-gate-blast-radius` | observed, **risk binding** — `best_practices_constraint_gate.rs` pins `discrete_choice.ri` as EXPECTED_INDETERMINATE and reds **both ways** once a solver runs. That belongs to δ's blast radius, not ω's, but ω will see it | PASS |

### Ω #6704 — PRD close

| Capability | Binding | Verdict |
|---|---|---|
| `every-leaf-upstream` | **DAG-direction** — Ω depends on all 14 siblings by real edges; a `cancelled` sibling counts as satisfied | PASS |
| `terminal-vocabulary-is-closed` | capability→producer — the three-value vocabulary and the freeze-header shape are fixed by the overlay and have committed exemplars (`data-carrying-enums.md`, `kernel-seam-contracts.md` at `edd9703fae`) | PASS |
| `artifacts-exist` | producer-self — the PRD and this manifest are committed by the authoring session, so Ω has both files to stamp | PASS |
