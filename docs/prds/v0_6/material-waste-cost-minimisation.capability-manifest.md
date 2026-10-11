# Capability manifest — material & waste cost minimisation (M-WASTE)

> **PRD:** `docs/prds/v0_6/material-waste-cost-minimisation.md`. **Status:** active — decomposed 2026-10-11,
> task ids stamped (α #8487 … ω #8504). The machine-readable twin is
> `material-waste-cost-minimisation.capability-manifest.yaml` (same bindings; its 21 mechanical
> `delivered_check`s were copied into the producer tasks at `commit_planning`). Evidence was gathered on main
> `df968c3c82` (2026-10-11) with a fresh debug `reify` built from that commit and a clean tree-sitter parser.
> Code anchors cite by symbol; main moves fast — re-locate at implementation time.

**Probe fixtures.**
- Committed (PRD evidence): `tests/prd-gate/fixtures/mwaste_{coupled_continuous,coupled_bool,let_constraint,objective_fallback,parent_auto_to_sub,fn_solid_no_fold}.ri`. All six reproduce PRD §2.3 exactly on `df968c3c82`.
- Ephemeral decompose probes under `/tmp/prd-gate-fixtures/mwaste/`, not committed (overlay fixture tiers):

| Probe | Result |
|---|---|
| `p_parent_concrete_to_sub.ri` | `Panel.a.t = 0.002 m`, `Panel.ma = 0.314 kg` |
| `p_enum_concrete_cost{,_ok}.ri` | `material_cost = 0.0942 USD`; the violated volume floor gives rc 1 |
| `p_realize_fail.ri` | `box(0mm,…)` gives rc 1, "box dimensions must be finite positive values" |
| `p_empty_difference.ri` | rc 0, `v = 0 m^3` |
| `p_pocket_volume.ri` | `v = 1.1685840734641023e-5 m^3` vs analytic `1.1685840734641021e-5` |
| `p_record_catalog.ri` | `n = 2`, `p1 = 42 USD` |
| `p_unregistered_builtin.ri` | rc 0, "unresolved function: nest_rectangles", `r = undef` |
| `p_enum_payload_records.ri` | named-field enum payloads and a `List<record>` param evaluate |
| `p_descendants_cost_filter.ri` | `filter(self.descendants, Flat)` gives `["Holder.a", "Holder.b"]` |

The manifest seats also used scratch C++ programs against the OCCT 7.8.1 headers (`AddOptimal` measurements).


## α — #8487

Geometry-coupling classification pass + coded refusals, on every path


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `classification-helper` | producer-self (§4.1, §8 α) — one helper classify_geometry_coupling(template, …) -> CouplingReport in crates/reify-eval, called from Engine::eval and Engine::eval_cached outside the has_active_solver gate and from the edit_param / edit_source warm builders. No such symbol, and no CouplingReport, exists on main (git grep rc=1, 2026-10-11). | **PASS** | grep `fn classify_geometry_coupling\b` present in `crates/reify-eval/src` |
| `sweep-state-type` | producer-self (§3 decision 11, §8 α names crates/reify-eval/src/lib.rs) — the engine-side SweepState record (not attempted / swept / infeasible / unevaluated / not run {reason}) that the pass consults so nothing is double-reported. Absent on main (git grep rc=1). Variant spellings are not bound; only the type name is. | **PASS** | grep `(enum\|struct) SweepState\b` present in `crates/reify-eval/src` |
| `post-realisation-read-detectors` | existing-substrate — geometry_ops::is_geometry_query_call (volume \| area \| centroid \| bounding_box) and is_geometry_consumer_call (both pub(crate), crates/reify-eval/src/geometry_ops.rs); body_mass_props is NOT call-shaped to those predicates: it is a name-recognised FunctionCall typed MassProperties whose value a build post-process writes via dynamics_ops::try_eval_body_mass_props; cross-sub cells are folded by Engine::post_process_cross_sub_value_cells (engine_build.rs, private). The classifier therefore needs its own read-cone walk over four read shapes, not a call-name list (as §4.1 already says). | **PASS** | manual: B1 and B3 (coupling through let indirection and Physical.mass) assert the classifier sees each read shape. |
| `classification-outside-solver-gate` | existing-substrate — Engine::eval and Engine::eval_cached each compute has_active_solver and gate only problem building on it; several analysis passes (cycle warnings, scope-coupling detection, robustness-floor detection) are already placed OUTSIDE that gate in engine_eval.rs, the precedent placement for the pass. | **PASS** | manual: B15 (no-solver and no-kernel paths) and B1 under reify check cover the placement. |
| `warm-builders-build-autos-inline` | existing-substrate, shape correction — Engine::edit_param and Engine::edit_source (crates/reify-eval/src/engine_edit.rs) do NOT call build_solver_problem, build_merged_solver_problem or build_auto_param_list. Each builds its AutoParam list and ResolutionProblem inline from `entity_groups: HashMap<String, (Vec<AutoParam>, HashSet<ValueCellId>)>` over `graph.value_cells` filtered by kind.is_auto(). So the 'one shared helper' of §4.1 must be called from two inline sites that today iterate a HashMap (B16 determinism). | **PASS** | manual: B16 asserts an unswept coupled template edited through edit_param gets the same classification. |
| `decomposition-is-hash-ordered` | existing-substrate — reify_constraints registry DecompositionPrelude / decompose_into_components assemble components by iterating a HashMap keyed on union-find roots (documented on the struct: component ORDER varies between invocations); decompose cannot see sub ctor-arg edges. Confirms the pass must not route through it. | **PASS** | manual: B4b and B19 (run twice, bit-identical) cover determinism; #6691 owns the leak itself. |
| `delta-trace-predicate-and-declined-set` | existing-substrate — engine_fixpoint.rs::constraints_reaching_auto (returns HashSet<ConstraintNodeId>) feeds both unresolved_diagnostics (E_EVAL_UNRESOLVED) and the `declined: &HashSet<ConstraintNodeId>` parameter of Engine::check_constraints_post_geometry (engine_constraints.rs). Its doc states the known limitation: it follows realization->realization edges only and does not chase a non-auto let that depends on an auto — which is exactly mwaste_let_constraint.ri (lead 2026-10-11: rc 1, 'max absolute residual: 1.00e0', no E_EVAL_UNRESOLVED). | **PASS** | manual: B3 asserts the let-indirected constraint is classified, with no residual-1.0 error. |
| `objective-inert-code-and-compile-rule` | existing-substrate — DiagnosticCode::ObjectiveInert exists in crates/reify-core/src/diagnostics.rs (Error severity, mnemonic E_OBJECTIVE_INERT) and is emitted by the COMPILE post-pass crates/reify-compiler/src/compile_builder/post_passes/inert_objective.rs. | **PASS** | — |
| `objective-inert-runtime-emission` | producer-self (§8 α lists 'E_OBJECTIVE_INERT for an empty S' in engine_eval.rs; resolution at decompose 2026-10-11: the signal moves to its producing leaf). PRD §4.3 step 1 / §5 B4b say the objective 'gets the existing E_OBJECTIVE_INERT'; the CODE exists (DiagnosticCode::ObjectiveInert, Error) but its only emitter is the compile post-pass crates/reify-compiler/src/compile_builder/post_passes/inert_objective.rs, whose is_data_transparent bails on any call node, so `minimize volume(body)` never fires it: measured on main df968c3c82, reify check and reify eval of tests/prd-gate/fixtures/mwaste_objective_fallback.ri print no E_OBJECTIVE_INERT (rc 0, x = undef, NoProgress), and the engine-side unconsumed-objective report defers 'reaches no auto' to the compile half. α emits the existing code engine-side when the classifier finds S empty; Error severity, so that fixture's eval exit flips 0 -> 1, consistent with the compile rule. | **PASS** | manual: B4b asserts the runtime emission on mwaste_objective_fallback.ri. Note the code is Error severity, so that fixture's reify eval exit flips from 0 to 1. |
| `fallback-component-zero-misroute` | existing-substrate baseline — reify_constraints ObjectiveConsumption::FallbackComponentZero (registry.rs) attaches an objective whose reads reach no auto to component 0. Measured 2026-10-11 on mwaste_objective_fallback.ri (lead; re-run by this seat): reify eval rc 0, x = undef, 'Constraint solver made no progress: objective expression evaluated to undefined at solution point'. | **PASS** | manual: B4b asserts x resolves exactly as with the objective deleted. |
| `undef-cause-for-not-resolved` | producer-self (§8 α names crates/reify-ir/src/value.rs) — UndefCause today has Unbound, AwaitingSolve, SolveFailed, OpContractFailed, UserUndef; α adds a variant (or reuses AwaitingSolve with a typed reason) for the not-resolved cause. The spelling is not fixed by the PRD. | **PASS** | manual: B1, B2, B15 assert the per-auto cause. Variant name is a working name. |
| `coupling-diagnostic-codes` | producer-self (§4.4, §11 Q2) — E_GEOMETRY_COUPLED_CONTINUOUS, E_GEOMETRY_SWEEP_SCOPE_UNSUPPORTED, W_GEOMETRY_COUPLED_NOT_RESOLVED in crates/reify-core/src/diagnostics.rs. Spellings are working names; none exists on main. | **PASS** | manual: B1, B2, B12, B15 assert the codes; spellings are §11 Q2. |
| `solverless-and-kernelless-paths` | existing-substrate — LSP (reify-lsp analysis.rs and diagnostics.rs: Engine::new(checker, None)), reify test (reify_eval::run_tests in test_runner.rs: Engine::new(checker, None)) and the reify-cli MCP ensure_engine (mcp_context.rs: Engine::new(SimpleConstraintChecker, None)) run with neither solver nor kernel. reify check builds Engine::with_registered_kernel or Engine::new(None) and never calls with_solver; cmd_build documents its solver-free posture in a NOTE at the engine construction. Only cmd_eval wires the full solver through configured_eval_engine. | **PASS** | manual: B15 runs B5's fixture under each of these paths. |
| `b4-no-coupling-regression-fixtures` | existing-substrate — crates/reify-eval/tests/fixtures/objective_set_weighted.ri (two Length autos, linear objective, no geometry read; included by tests/harness_fea_solver_e2e/objective_provenance.rs). Merged-cluster: examples/whole_model_cost_min.ri (minimize cost(self.descendants) over two children with closed-form Money costs, no geometry read). crates/reify-eval/tests/merged_cluster_solve.rs carries its sources inline, not as a .ri file. | **PASS** | manual: B4 asserts byte-identical values and diagnostics (W1). |
| `baseline-fixtures-committed` | existing-substrate — tests/prd-gate/fixtures/mwaste_{coupled_continuous,coupled_bool,let_constraint,objective_fallback,parent_auto_to_sub,fn_solid_no_fold}.ri are tracked. The lead measured all six against PRD §2.3 on a fresh debug build of df968c3c82 on 2026-10-11 (continuous: eval rc 1 'objective expression evaluated to undefined', check rc 0 with error: lines; bool: rc 0, w = 0.04 m, wide = true, silent; let_constraint: rc 1 residual 1.00e0; objective_fallback: rc 0, x = undef; parent_auto_to_sub: rc 1 'per-instance re-realization compile error for Panel.a.geometry'; fn_solid_no_fold: v_fn undef, ex_inline = 0.0400002 m). No fixture for the B5-B9 finite sweeps and none for the §2.3 row-7 sub-object shape (B19) exists yet; those are authored by γ/η. | **PASS** | manual: B1, B2, B3, B4b assert the polarity flip against these baselines. |

## β — #8488

Auto-pin overlay


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `classification-upstream` | producer-upstream — α (CouplingReport and S in declaration order; SweepState). Edge α → β per §8. | **PASS** | — |
| `sweep-pins-field` | producer-self (§3 decision 11, §4.2) — Engine.sweep_pins : BTreeMap<ValueCellId, Value>, distinct from param_overrides, written only by the sweep. Absent on main (git grep rc=1). | **PASS** | grep `sweep_pins:\s*(std::collections::)?BTreeMap` present in `crates/reify-eval/src` |
| `sweep-pins-must-survive-row` | producer-self (§4.2) — Engine::reset_per_build_state (crates/reify-eval/src/engine_build.rs) opens with an EXHAUSTIVE `let Engine { .. } = self` destructure with no `..`, so a new Engine field cannot compile until classified; sweep_pins is a MUST-SURVIVE row bound `sweep_pins: _` like param_overrides and eval_state. Verified the exhaustive form on main 2026-10-11. | **PASS** | grep `sweep_pins:\s*_` present in `crates/reify-eval/src/engine_build.rs` |
| `param-overrides-covers-param-cells-only` | existing-substrate — eval's unguarded and guarded-group arms consult param_overrides only when `cell.kind == ValueCellKind::Param`; Auto cells are pre-seeded and re-served in their own arm, so an override on an auto is never read (the premise for a separate sweep_pins). | **PASS** | manual: B14 asserts a pinned auto behaves as a concrete param through subs. |
| `merged-builder-refuses-pins` | existing-substrate — engine_eval.rs::connector_pin_if_determined (pub(crate)) excludes a determined connector from auto_params only inside build_solver_problem and only under has_active_solver; build_merged_solver_problem instead pushes an Error for a strict connector-instance auto contributed to a merged cluster (tasks #5014, #4710). β must make the merged builder exclude sweep pins instead of erroring. | **PASS** | manual: B4 (merged-cluster fixture unchanged) plus β's in-src test of the merged builder with a pin. |
| `auto-predicate-site-inventory` | existing-substrate, count correction — in non-test, non-comment code of crates/reify-eval/src there are 33 lines using `.is_auto()` or `is_auto_cell(` (32 consumers plus the body of EvaluationGraph::is_auto_cell), 5 lines using `is_auto_free()`, and 3 structural `ValueCellKind::Auto { .. }` pattern sites that the predicate grep does NOT find (the exhaustive-match arm in the engine_eval.rs guard-deactivation block, engine_purposes.rs, structural_classifier.rs). PRD §2.4 says 'some two dozen'. Every §4.2 named site was located: is_auto_cell, evaluate_params_and_lets_unified, Snapshot::from_compiled_module, build_auto_param_list, build_combined_param_let_graph, CellReadIndex::build, scope_qualifies_for_centrality, detect_underdetermined, resolve_order, deactivate_if_not_auto, deps.rs, module_has_auto_cells, invariants.rs, the re_eval_consumers_of_in_walk_mints family. | **PASS** | manual: β's own description carries the per-site classification (§4.2); B14 and the in-process signal cover behaviour. |
| `pre-seed-outside-solver-gate` | producer-self (§4.2, §3 decision 12) — pins applied as concrete values at the auto pre-seed in Engine::eval, Engine::eval_cached and Snapshot::from_compiled_module, outside has_active_solver and before child elaboration, so a pinned parent auto reaches a sub's ctor-arg overlay. Baselines (lead 2026-10-11): a concrete parent param reaches the overlay (p_parent_concrete_to_sub.ri: rc 0, Panel.a.t = 0.002 m, Panel.ma = 0.314 kg); a solved parent auto does not (mwaste_parent_auto_to_sub.ri: rc 1, per-instance re-realization compile error). | **PASS** | manual: B14 asserts the pinned file matches the concrete-true file and a geometry constraint over a pinned auto gets a definite verdict. |
| `warm-resolution-skips-pinned` | producer-self (§4.2 'warm edits never re-solve or override a pinned cell') — the two inline AutoParam sites in engine_edit.rs (edit_param, edit_source) and deactivate_if_not_auto must treat a pinned auto as determined. | **PASS** | manual: β's in-process signal: a pinned auto is skipped by the warm re-solve; B16 and B13 exercise it end to end. |
| `single-pin-aware-predicate` | producer-self (§4.2, W5) — one pin-aware 'is this an unresolved auto?' predicate replacing the site inventory. Name unspecified by the PRD. | **PASS** | manual: predicate name is tactical; coverage is the per-site classification in β's description. |

## γ — #8489

The sweep: engine entry, enumeration, candidate evaluation, ranking, honesty; reify eval wiring


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `classification-and-pins-upstream` | producer-upstream — α (classification, SweepState) and β (sweep_pins overlay) per §8; edges α → γ and β → γ. | **PASS** | — |
| `domain-channel-upstream` | producer-upstream — #5470 (pending; PRD 2 δ): AutoParam.domain, AutoParam::is_discrete(), Enum DECLARED domain, Int bound-mining. Baseline verified on main 2026-10-11: reify_ir AutoParam has fields id, param_type, bounds, free only; engine_eval.rs::build_auto_param_list sets bounds: None; cpsat.rs::domain_spec accepts Bool intrinsically, rejects an Int with no bounds, and mines an Enum domain from variant literals in the constraints it is passed. Without #5470 only Bool enumerates and a stripped constraint set narrows an Enum (B9). Hard edge #5470 → γ wired at decompose. | **PASS** | — |
| `solve-all-enumeration` | existing-substrate, shape correction — CpSatSolver::solve_all(&ResolutionProblem, cap) is pub and re-exported with SolveAllResult from reify_constraints. SolveAllResult is an ENUM, not the struct the PRD and brief sketch: Enumerated { solutions: Vec<HashMap<ValueCellId, Value>>, complete: bool } \| NotEnumerable { reason: String }. The sweep must map NotEnumerable to the 'no declared finite domain' refusal; { solutions: [], complete: true } is a proof of pre-geometry infeasibility (#8356's NoCatalogMemberFeasible), { [], false } proves nothing. solve_all_with_budget is pub(crate). | **PASS** | manual: B8 and B9 assert completeness and the declared domain. |
| `enumeration-cap-vs-node-budget` | existing-substrate — the cap is checked at the push, so the cap-th solution reports complete: false even if it was the last (deliberately conservative); with cap = budget + 1 a domain of exactly `budget` candidates reports complete: true (§3 decision 8 holds). A SEPARATE node budget (ENUMERATION_NODE_BUDGET = 100_000 in cpsat.rs) also yields complete: false with FEWER than budget + 1 solutions, so 'incomplete' does not imply 'budget candidates were returned'. | **PASS** | manual: B8 covers below-budget, exactly-budget and over-budget domains. |
| `ranking-helpers-reusable` | existing-substrate — ScoredModel (private struct, Ord by score then enumeration index), tie counting and verdict_from_enumeration (private fn) live in crates/reify-constraints/src/cpsat.rs and are reached only through solve_ranked_with_budget. γ factors them for reuse (§4.3 step 5); none is pub today. | **PASS** | manual: B8 and B11 assert ranking order and tie handling; W5 forbids a second copy. |
| `objective-fold-pub` | producer-self (§8 γ) — reify_constraints::solver::eval_objective_set is pub(crate) on main, returns Result<f64, ObjectiveAbstention>. Making it `pub fn` is NECESSARY BUT NOT SUFFICIENT for reify-eval: `mod solver` is private in reify-constraints/src/lib.rs (only DimensionalSolver and build_centrality_objective are re-exported) and ObjectiveAbstention is itself pub(crate). γ must also make ObjectiveAbstention pub and re-export both from the crate root. | **PASS** | grep `pub fn eval_objective_set\b` present in `crates/reify-constraints/src` |
| `optimality-gate-reused` | producer-self (§8 γ) — engine_eval.rs::optimality_unproven_finding is a private `fn` on main; γ makes it pub(crate) so the sweep calls the one gate (W_SOLVER_OPTIMALITY_UNPROVEN fires iff BestFound{reason} with reason.stopped_at_budget(); ConvergedWithinBudget and Unreported stay silent). | **PASS** | manual: W5 (one optimality gate) is judged in review and by B8 (W_SOLVER_OPTIMALITY_UNPROVEN exactly once when capped). Not grep-bound: the pub(crate) change is avoidable if the sweep's finding is built inside engine_eval.rs, so a visibility grep could stay red after a correct delivery and wedge γ's dependents. |
| `candidates-unevaluated-variant` | producer-self (§4.3, §7, §8 γ) — additive PartialReason::CandidatesUnevaluated { count } in crates/reify-ir/src/completeness.rs. Verified absent on main (the only hits are in the PRD). Existing variants: BoxBudgetExhausted, DimensionAboveEnvelope, DomainUnbounded, NotIntervalRepresentable, InnerSolveUnproven, ProbeOnly, NotAttempted. Completeness::{Exhaustive, Partial, Refuted}, derived_unique and permits_proven_optimal exist and are re-exported from reify_ir. | **PASS** | grep `^\s*CandidatesUnevaluated\s*[{(]` present in `crates/reify-ir/src/completeness.rs` |
| `sweep-builds-its-own-completeness` | existing-substrate, premise correction — PRD §3 decision 6 and §2.4 say P3 'maps a capped CP-SAT enumeration to Partial{BoxBudgetExhausted}'. On main CpSatSolver::solve_ranked still reports Completeness::not_attempted() on both Ranked returns; that mapping is not landed. The sweep does not depend on it: it constructs Completeness::{Exhaustive, Partial{..}} itself over the pub reify_ir enum, with RankedSolveResult::Ranked { candidates, optimality, completeness } and BestFoundReason::{EnumerationBudget, ConvergedWithinBudget} as landed. | **PASS** | manual: B8 (capped: BestFound{EnumerationBudget}, one warning) and B10 (Partial{CandidatesUnevaluated}, never ProvenOptimal). |
| `build-pipeline-and-nonemitting-variant` | existing-substrate — Engine::build and Engine::realize_for_check both delegate to the private build_with_geometry_output(module, format, emit_geometry_output); realize_for_check passes false. That worker runs reset_per_build_state, self.check(module), execute_realization_ops, run_post_processes and check_constraints_post_geometry (UnifiedDag-gated). The non-emitting flag exists; the new part is returning score, per-constraint verdicts and residual-solve status. `module_named_steps` is a local map (HashMap<String, HashMap<String, KernelHandle>>), not a function. | **PASS** | manual: B5, B6, B7 assert scores and verdicts through reify eval. |
| `entry-outside-build-reentry` | existing-substrate — relate_solve::realize_structures realises a filtered sub-module by calling engine.build(&sub_module, ExportFormat::Step), which reaches build_with_geometry_output; a sweep entered from inside it would recurse, so the entry is called by the caller before its normal path (§4.3). The re-entry is via Engine::build, not a direct build_with_geometry_output call. | **PASS** | manual: γ's cmd_eval wiring test asserts the entry runs once, before realize_for_check, and that the sweep does not recurse. |
| `per-candidate-geometry-reset` | existing-substrate, scope caveat — Engine::reset_geometry_for_reload (engine_admin.rs) resets every registered kernel and clears the realization cache. GeometryKernel::reset has a NO-OP default; only the OCCT kernel and reify_geometry's SingleKernelHolder override it (the trait doc says ManifoldKernel, OpenVdbKernel and FidgetKernel each still need their own override). The OCCT-memory bound per candidate therefore holds when the default kernel is OCCT, which is the BRep-preferring pick for reify eval. | **PASS** | manual: B19 (two runs bit-identical) plus the reset-between-candidates in-src test. |
| `cmd-eval-geometry-branch` | existing-substrate — crates/reify-cli/src/main.rs::cmd_eval, on module_has_geometry, builds configured_eval_engine(Engine::with_registered_kernel(..)) (adds with_solver(SolverRegistry::production())) and calls engine.realize_for_check(&compiled). cmd_eval rejects unknown flags while parsing, so --sweep-budget needs an explicit arm. Scheduler: BuildScheduler::from_env defaults to UnifiedDag (REIFY_BUILD_SCHEDULER=legacy is the kill-switch). | **PASS** | manual: B5-B8 run under reify eval; --sweep-budget spelling is §11 Q1. |
| `sweep-entry-name-and-diagnostic-codes` | producer-self — Engine::resolve_geometry_coupled (working name, §4.3), --sweep-budget (§11 Q1), I_GEOMETRY_SWEEP, E_GEOMETRY_SWEEP_INFEASIBLE, E_GEOMETRY_SWEEP_UNEVALUATED (§4.4, §11 Q2). None exists on main. | **PASS** | manual: working names; B5, B7, B8, B10 assert the behaviour. |
| `robustness-floor-exclusion` | existing-substrate — engine_eval.rs::detect_robustness_floor_applied (private fn, runs outside the solver gate) emits the PRD 1 floor-applied info line; γ excludes the swept scope from it (§3 decision 15). | **PASS** | manual: B5 asserts no floor-applied info on a swept scope. |
| `b10-fixture-shape-correction` | fixture-shape correction (lead measurement 2026-10-11) — PRD §5 B10's example 'a boolean that empties at one parameter value' is NOT a realisation failure: difference(box 10, box 20) evaluates rc 0 with v = 0 m^3 (p_empty_difference.ri), so it is a feasible candidate. The failing shape is a zero or negative box dimension at one candidate (p_realize_fail.ri: box(0mm, …) gives rc 1 'box dimensions must be finite positive values', v undef). Use that to produce an UNEVALUATED candidate. This is a fixture correction, not a design failure. | **PASS** | manual: B10 asserts unevaluated vs infeasible vs proven using the corrected fixture shape. |
| `determinism-declaration-order` | existing-substrate — build_auto_param_list maps the template's value_cells slice in declaration order (no HashSet), and solve_all documents declaration-order variables and construction-order values with no RNG or clock; OCCT volume is bit-identical run to run (lead probe, PRD §2.1). | **PASS** | manual: B19 runs B5/B6 and a finite sub-object fixture twice and asserts bit-identical values and summary. |

## γ2 — #8490

Sweep verdict diagnostics per P3 §3.4


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `sweep-upstream` | producer-upstream — γ (the sweep commit step and its verdict). Edge γ → γ2 per §8. | **PASS** | — |
| `policy-function-absent-on-main` | existing-substrate (absence) — no §3.4 verdict-to-diagnostic policy function exists on main as of 2026-10-11: no function in engine_eval.rs, reify-ir completeness.rs or ranked.rs maps tie / Partial to a coded diagnostic. The nearest code is push_merged_cluster_nonunique_warnings (engine_eval.rs), the generic auto(free) not-uniquely-determined warning, and the ConstraintNonUnique code. #6901 is pending, so γ2 may land first and then owns the function (§3 decision 7). | **PASS** | manual: location and name are §11 Q6 (whoever lands first implements, the other adopts); no mechanical binding is safe. |
| `tie-semantics` | existing-substrate — tie counting in CpSatSolver::solve_ranked_with_budget (cpsat.rs) is exact f64 equality of scores with enumeration index as tiebreak; §3 decision 7 reuses that. | **PASS** | manual: B11 uses two candidates with exactly equal scores, strict and auto(free). |
| `verdict-diagnostic-codes` | producer-self — strict-tie Error, free-tie Warning, Partial Warning codes (§4.4 'with #6901'). Spellings follow whichever of #6901 and γ2 lands first. | **PASS** | manual: B11 asserts severity and wording content; codes are §11 Q2. |

## δ — #8491

GUI: load / commit / Export see the sweep; drags keep the winner; cancellation


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `sweep-upstream` | producer-upstream — γ (engine entry, SweepReport, pins). Edge γ → δ per §8. | **PASS** | — |
| `eval-diagnostics-field-upstream` | producer-upstream — #6724 (pending; P4 γ: route eval-time diagnostics into the GUI) adds the eval-diagnostics field on GuiState. Verified absent on main 2026-10-11: GuiState (gui/src-tauri/src/types.rs, macro-declared) carries compile_diagnostics and tessellation_diagnostics only. Hard edge #6724 → δ wired at decompose. | **PASS** | — |
| `gui-load-and-reload-funnel` | existing-substrate — gui/src-tauri/src/engine.rs: EngineSession::with_registered_kernel installs SolverRegistry::production() plus the registered kernel (production boot only; EngineSession::new stays solver-free). load_file, load_from_source and update_source all funnel through the private check_with_solve_slot, which calls reset_geometry_for_reload and then with_solve_slot(\|s\| engine.check(compiled)); tessellate_snapshot follows in build_gui_state. | **PASS** | manual: B13 asserts load, drag and commit through the debug-server harness. |
| `export-builds-current-state` | existing-substrate — EngineSession::export refuses an unenforced representation bound and then calls engine.build(compiled, format) on the engine's current state; it does not call check_with_solve_slot, so pins left by the last load/commit are what gets built. | **PASS** | manual: W7 (the winner is what is exported) is asserted by B13's follow-up and the γ/δ in-src export test. |
| `per-solve-cancellation-handle` | existing-substrate — with_solve_slot mints a fresh CancellationHandle per check, installs it via Engine::set_active_solve_cancel and clears it afterwards; Engine::active_solve_cancel is a pub accessor in reify-eval engine_admin.rs. The sweep call must sit inside with_solve_slot to see the handle. | **PASS** | manual: cancel-between-candidates is asserted by δ's in-src test with a pre-cancelled handle (pins nothing, not-resolved cause). |
| `slider-vs-commit-paths` | existing-substrate — EngineSession::preview_parameter calls edit_check (Engine::edit_param) inside with_solve_slot and deliberately BYPASSES check_with_solve_slot (no kernel reset); commit_parameter -> apply_param_to_source_str -> update_source -> check_with_solve_slot, so a commit re-sweeps. The debug-MCP reify_set_parameter handler calls commit_parameter. | **PASS** | manual: B13 drag-then-commit sequence. |
| `debug-server-harness-for-b13` | existing-substrate — gui/src-tauri/src/debug_server.rs (tools engine_state, reify_set_parameter, …) with child test modules in gui/src-tauri/src/debug_server/tests/{advertised_params,rebuild_cost,write_tools}.rs; built only under the `gui` feature (links OCCT): cargo test -p reify-gui --lib --features gui. CAVEAT: the debug-MCP surface has no preview/drag tool, only the commit-shaped reify_set_parameter; a slider drag in B13 must be driven in-process through commands::preview_parameter_impl on the shared Arc<Mutex<EngineSession>> (debug_server.rs's own tests already do this), not over the MCP wire. | **PASS** | manual: B13 is the leaf signal; no mechanical binding because every δ name (I_GEOMETRY_SWEEP_STALE, the #6724 field) is a working name or an upstream deliverable. |
| `stale-info-diagnostic` | producer-self (§4.4, §8 δ) — I_GEOMETRY_SWEEP_STALE emitted from edit_param when an edit touches an input of T, surfaced through #6724's field; pins cleared on update_source / reload. Working name (§11 Q2). | **PASS** | manual: B13 asserts presence after the drag and absence of value change. |

## ε — #8492

M-WASTE ε: exact bounding extents (BRepBndLib::AddOptimal) + stock vocabulary (SheetStock record, Stocked / SheetPart traits)


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `query-bbox-pads-today` | existing-substrate baseline (polarity flip ε delivers) — crates/reify-kernel-occt/cpp/occt_wrapper.cpp::query_bbox calls BRepBndLib::Add(shape.shape, box) (default useTriangulation, shape tolerance included). Measured 2026-10-11 against the installed OCCT 7.8.1 with a scratch C++ program: a 40x30x10 mm box gives Add extents 0.0400002 / 0.0300002 / 0.0100002 m, so blank_volume = 1.2000380003e-5 m^3 vs exact 1.2e-5 (3.1667e-5 relative). `reify eval` of a Stocked-shaped trait over the pocketed block (scratch p_stocked.ri) printed extent_x = 0.0400002 m and offcut_volume = 3.14539e-7 m^3 against the analytic 100*pi mm^3 = 3.14159e-7 (1.2e-3 relative). | **PASS** | manual: baseline evidence for the polarity flip; B17 asserts the post-ε values. |
| `addoptimal-declared-in-installed-header` | existing-substrate — /usr/include/opencascade/BRepBndLib.hxx (OCC_VERSION_COMPLETE 7.8.1) declares `static void AddOptimal(const TopoDS_Shape& S, Bnd_Box& B, const Standard_Boolean useTriangulation = Standard_True, const Standard_Boolean useShapeTolerance = Standard_False)`. No AddOptimal call exists anywhere in the tracked tree (git grep rc 1 outside the PRD). | **PASS** | — |
| `query-bbox-switches-to-addoptimal` | producer-self (§4.5) — query_bbox in crates/reify-kernel-occt/cpp/occt_wrapper.cpp calls BRepBndLib::AddOptimal(shape.shape, box, Standard_False, Standard_False). is_bounded keeps its own BRepBndLib::Add (second site, a predicate, deliberately out of scope), so the positive AddOptimal call is the check, not the removal of Add. | **PASS** | grep `BRepBndLib::AddOptimal\(` present in `crates/reify-kernel-occt/cpp/occt_wrapper.cpp` |
| `exact-extents-on-analytic-surfaces` | floor: B17's two shapes. Independent scratch measurement (OCCT 7.8.1, AddOptimal(shape, box, false, false)): 40x30x10 mm box extents exact (blank = 1.1999999999999999e-5, offcut/volume = -1.41e-16); pocket difference(box, cylinder(5mm,4mm)) offcut/(pi r^2 h) - 1 = -1.27e-14; sphere, cone and cylinder extents exact. The signal to distinguish is the padding: 3.1667e-5 x volume on the box and 1.21e-3 relative offcut error on the pocket. Bounds 1e-10 (box) and 1e-9 (pocket) sit 5-6 orders above the residual and 5-6 orders below the padding signal. | **PASS** | manual: B17 numeric assertions in the ε leaf tests (harness_engine module); the 1e-10 / 1e-9 bounds are the floor. |
| `exact-extents-scope-analytic-faces` | existing-substrate measurement, premise SCOPED at decompose (G6 resolution: scope the claim; the B17 bound stands) — AddOptimal(shape, box, false, false) is exact on analytic faces (box offcut/volume -1.4e-16, pocket -1.27e-14, sphere, cone, cylinder; scratch C++ against OCCT 7.8.1 headers, 2026-10-11) but NOT on a torus (R 10 mm, r 2 mm: zmax = 0.0020001, still +1e-7; 25.2 ms vs 0.004 ms for Add, ~6000x) nor a NurbsConvert'd cylinder (+1e-7 per side; 0.447 ms vs 0.019 ms, 23x). Add on the torus overshoots far worse (xmax 0.0129888 vs true 0.0120), so AddOptimal remains the better query. Consequences: B17 (box + pocket, analytic) holds; stock-fit is exact for sheet parts built from box / extrudes of line-and-arc profiles; a part with torus or B-spline faces keeps a ~1e-7 m per-side pad, so ε records these figures with its §11 Q7 timing ratio (and keeps Add for any selector-internal use the cost makes material), and θ1 scopes 'exact' to analytic faces and tells authors to compare stock fit with a lattice tolerance (1 µm, as ζ's nesting lattice does) for curved-face parts. Surfaced to Leo in the decompose hand-back. | **PASS** | manual: ε's description records the torus / NURBS residuals and timing; θ1's chunk text scopes the claim. |
| `manifold-bbox-adds-no-gap` | existing-substrate (code-read, not measured here) — crates/reify-kernel-manifold/src/kernel.rs GeometryQuery::BoundingBox delegates to Manifold::bounding_box() for a whole mesh and to face_points_bbox / points_bbox for a sub-shape; no gap is added in reify code. Tightness is with respect to mesh vertices (a faceted curved shape understates the true extent), which is the existing Manifold behaviour. | **PASS** | manual: crates/reify-kernel-manifold/tests/queries.rs covers the query arm; ε changes nothing there. |
| `bbox-consumers-exist-and-are-reverified` | existing-substrate — every consumer the PRD names exists: geometry_ops.rs::dispatch_bounding_box (-> Value::BoundingBox), crates/reify-compiler/stdlib/process.ri fits_build_volume(bounding_box(part), bounding_box(proc.build_volume)) with crates/reify-stdlib/src/dfm.rs, engine_build.rs cap-vertex (GeometryQuery::BoundingBox(cap_vertex_handle)) and new-edge-midpoint uses plus harvest_dfm_let_diagnostics, primitive_attribute_seed.rs, topology_selectors.rs (edges_at_height) and selector_vocabulary_v2.rs (extremal_by_bbox), crates/reify-eval/tests/centered_primitives_e2e.rs (bbox_tol 1e-6), and the harness_occt_measurement extrude_symmetric_integration.rs / loft_guided_integration.rs tests. The PRD's list is NOT complete: also crates/reify-eval/tests/rounded_primitives_e2e.rs, harness_topology_selector/topology_filtered_selectors.rs (1e-6 tolerance justified by BRepBndLib::Add), crates/reify-cli/tests/harness_cli/cli_check.rs (E_DFM_BUILD_VOLUME build-volume fit), crates/reify-eval/tests/common/differential.rs (fits_build_volume), crates/reify-kernel-occt/src/lib.rs in-src tests (BBOX tolerances 1e-6 / 1e-5), tests/harness_occt/{step_document_integration.rs (BBOX_TOL), apply_transform_integration.rs, conformance_integration.rs}, tests/harness_occt_measurement/{boolean_result_normalization_integration.rs, per_edge_chamfer.rs, topology_extract_integration.rs, reflection_det_negative_integration.rs}. A third BRepBndLib::Add site exists in occt_wrapper.cpp (boolean_operand_solids_are_pairwise_disjoint, an IsOut separation predicate) and is out of scope for the same reason as is_bounded. | **PASS** | manual: every pre-existing bounding_box consumer test green is part of B17's postcondition; the added consumers above are for ε's diff to re-verify. |
| `stdlib-registration-slot` | existing-substrate — crates/reify-compiler/src/stdlib_loader.rs registers modules as (name, include_str!("../stdlib/<file>.ri")) pairs in a hand-ordered list; std.determinacy.purposes is last and every comment says new modules go before it. No stock.ri exists (git ls-files rc 1) and none of SheetStock / Stocked / SheetPart / NestStatus / NestResult / NestGroup / PartFootprint / nest_rectangles / nest_sheet_parts appears anywhere in tracked files outside the PRD (git grep rc 1). Caveat: the module name `std.stock` is already taken by standard_stock.ri, so the new module needs a distinct name; the duplicate-pub-name scan (NS-P2) forbids a cross-module name clash. | **PASS** | — |
| `sheet-stock-record` | producer-self (§4.5) — structure def SheetStock with members material : String, thickness : Length, width : Length, length : Length, price : Money in crates/reify-compiler/stdlib/. Authoring correction (measured 2026-10-11, reify check): the PRD's one-line `structure def X { param a : T; param b : U }` form is a parse error ('syntax error in structure body'); the stdlib file must declare one member per line. A list of structure records in a structure let works: [SheetStock2(material: "Steel", thickness: 2mm, width: 1000mm, length: 2000mm, price: 40USD)] evaluated, cat[0].price = 40 USD (scratch p_stocked.ri, and the lead's p_record_catalog.ri: cat.count = 2, cat[1].price = 42 USD). No price list ships (decision 13). | **PASS** | grep `structure def SheetStock\b` present in `crates/reify-compiler/stdlib` |
| `stocked-and-sheetpart-traits` | producer-self (§4.5) — trait Stocked : Physical with extent_x/y/z, blank_volume, offcut_volume lets and trait SheetPart : Stocked with param quantity : Int = 1. Physical (crates/reify-compiler/stdlib/structural_physical.ri) is trait Physical { param geometry : Solid; param material : Material; let mass = volume(geometry) * material.density; let centroid; constraint material.density > 0kg/m^3 }. Authoring correction: a bodyless trait let (`let extent_x : Length;`, as §4.5 abbreviates it) is a parse error; each let needs its body. Measured design shape (scratch p_stocked.ri, reify eval, debug binary of the same tree): `let extent_x : Length = dot(bbox_size(bounding_box(geometry)), vec3(1, 0, 0))` and blank_volume / offcut_volume built from three inline extents fold at the scoped id for a non-collection sub (Panel.a.extent_x = 0.0400002 m, Panel.a.blank_volume = 1.2000380003e-5 m^3, Panel.a.offcut_volume = 3.14539e-7 m^3), and the parent read `let tot = self.a.blank_volume` folds. | **PASS** | grep `trait SheetPart\s*:\s*Stocked` present in `crates/reify-compiler/stdlib` |

## ζ — #8493

M-WASTE ζ: rectangular nesting builtin nest_rectangles (EvalBuiltin row, MaxRects kernel) + status pass


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `epsilon-upstream` | producer-upstream — ε (stock.ri types SheetStock / Stocked / SheetPart; exact extents). | **PASS** | — |
| `evalbuiltin-registry-rows-wired` | existing-substrate — crates/reify-builtins/src/registry.rs declares `registry! { EvalBuiltin => EvalBuiltinId, as_eval_builtin { ... } }` with seven rows today (parse_length, parse_length_r, von_mises, max_shear, principal_stresses, safety_factor, stress_invariants); crates/reify-stdlib/src/registry_dispatch.rs::dispatch is an exhaustive `match id` with no `_` arm, so a new row fails to compile until its arm exists; try_dispatch resolves via reify_builtins::lookup(name, argc). The stress_invariants row is the exact precedent for a struct-returning builtin: `result: Const(Type::StructureRef("StressInvariants".to_string()))` with the kernel (analysis.rs::stress_invariants) returning Value::StructureInstance(StructureInstanceData { type_name: "StressInvariants", .. }) for a .ri-declared structure. | **PASS** | — |
| `nesting-kernel-file-absent` | existing-substrate baseline — crates/reify-stdlib/src has no nesting.rs (git ls-files rc 1); lib.rs and registry_dispatch.rs exist and are the two files ζ edits besides registry.rs. | **PASS** | — |
| `unregistered-builtin-is-warning-only` | existing-substrate (measured by the lead, /tmp/prd-gate-fixtures/mwaste/p_unregistered_builtin.ri, 2026-10-11) — `let r = nest_rectangles([], [], kerf)` gives rc 0, `warning: unresolved function: nest_rectangles`, r = undef. No RED test may assert a hard error for the unregistered name. | **PASS** | — |
| `nest-rectangles-registry-row` | producer-self (§4.6) — a registry row with name: "nest_rectangles" in crates/reify-builtins/src/registry.rs (arity 3, result Const(StructureRef("NestResult"))). | **PASS** | grep `"nest_rectangles"` present in `crates/reify-builtins/src/registry.rs` |
| `nest-result-structure` | producer-self (§4.6) — structure def NestResult { status : NestStatus, cost : Money, sheets_used : Int, utilisation : Real, waste_area : Area, groups : List<NestGroup> } beside PartFootprint and NestGroup in the stdlib .ri. Authoring correction: one member per line (the §4.6 one-line `;` form is a parse error). | **PASS** | grep `structure def NestResult\b` present in `crates/reify-compiler/stdlib` |
| `nest-status-enum-with-payloads` | producer-self (§4.6) — enum NestStatus { Ok, NoStock { material: String, thickness: Length }, PartTooLarge { part: Int, largest_width: Length, largest_length: Length }, InvalidInput { reason: String } }. Existing-substrate for the shape: an enum with named-field payload variants plus a structure with a List<GroupProbe> param evaluates, rc 0 (lead's p_enum_payload_records.ri, 2026-10-11); declared multi-line it also type-checks in a scratch module. | **PASS** | grep `enum NestStatus\b` present in `crates/reify-compiler/stdlib` |
| `status-pass-slot-in-run-post-processes` | existing-substrate — crates/reify-eval/src/engine_build.rs::run_post_processes runs, in order, post_process_geometry_queries, post_process_topology_selectors, post_process_feature_datum_projections, post_process_cross_sub_value_cells, post_process_derived_lets (a fixpoint, so a parent let that calls nest_rectangles is re-evaluated once the child extent lets fold), harvest_dfm_let_diagnostics, post_process_ad_hoc_selectors, post_process_feature_accessor, then body_mass_props (documented LAST). Its doc comment carries a SYNC REQUIREMENT with Engine::hydrate_value_cell_in_loop (the UnifiedDag single-cell ladder), so ζ's pass must be mirrored there or only the whole-template path reports the E_NEST_* error. | **PASS** | manual: B18's no-stock half asserts the E_NEST_* Error and the UndefCause under reify eval (UnifiedDag); diagnostic code spellings are working names (PRD §11 Q2) and are deliberately not bound. |
| `b18-numeric-premises-hand-computable` | floor: see the arithmetic block. Four 500x500 mm squares on one 1000x1000 mm sheet, kerf 0 -> sheets_used 1, utilisation 1.0, cost 1.0 sheet price, waste_area 0. Kerf 2 mm -> parts inflate to 502, sheet to 1002; 502 + 502 = 1004 > 1002 so no two squares share a row or column: sheets_used 4, cost 4.0 sheet prices, utilisation (nominal areas) 0.25, waste_area 3 m^2. The count is algorithm-independent (it follows from 1004 > 1002), squares make orientation irrelevant, and it needs a SINGLE-entry sheet catalog: a larger catalog sheet would win under item 4's least-sheets_used x price rule. | **PASS** | manual: B18 explicit-list half in the ζ leaf tests; the expected counts are the 1 / 4 above, computed from §4.6 items 2-5, not taken from a run. |

## ζ2 — #8494

M-WASTE ζ2: nest_sheet_parts(filter(self.descendants, SheetPart), sheets, kerf) descendants form


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `zeta-upstream` | producer-upstream — ζ (nest_rectangles, NestResult, PartFootprint); ε (SheetPart trait, extent_* / quantity / material cells) through ζ. | **PASS** | — |
| `cost-descendants-intercept-and-rewrite` | existing-substrate — crates/reify-compiler/src/expr.rs intercepts `name == "cost" && args.len() == 1 && !functions.iter().any(\|f\| f.name == "cost")` before overload resolution and emits FunctionCall std::cost typed Scalar<MONEY>; crates/reify-eval/src/structural_query.rs::apply_cost_aggregation (pub(crate), signature (expr, all_templates, trait_registry) -> ()) keeps Costed conformers, rewrites each entity-ref literal to ValueRef(ValueCellId(path, "line_cost")) and wraps them in a .sum method call. It has NO diagnostics parameter today, so ζ2 must add a sink (the PRD's stated plan). apply_trait_filters has already reduced filter(self.descendants, Trait) to a ListLiteral of Literal(String path) typed StructureRef by the time the rewrite runs. | **PASS** | — |
| `filter-descendants-by-trait-compiles` | existing-substrate (measured by the lead, /tmp/prd-gate-fixtures/mwaste/p_descendants_cost_filter.ri, 2026-10-11) — filter(self.descendants, Flat) compiles (reify check rc 0) and evaluates to the entity-path strings ["Holder.a", "Holder.b"]; members cannot be read through the strings, hence the intercept. | **PASS** | — |
| `descendants-depth-and-collection-limits` | existing-substrate, with a correction to the PRD wording — structural_query.rs::enumerate_descendants DOES emit collection-sub instances (paths `Holder.s[i]`) and recurses transitively, so filter(self.descendants, SheetPart) lists them. The limit is on the geometry fold, not the enumeration: engine_build.rs::post_process_cross_sub_value_cells folds only the direct, non-collection sub_components of the template being processed (it `continue`s on sub.is_collection; scoped entity is `{template}.{sub}`), pinned by post_process_cross_sub_value_cells_skips_collection_subs. So a collection element's or a depth-2 descendant's extent_* cells stay Undef. cost() has no such limit only because line_cost is a pure cell elaborated for collection elements. 'Same limits as cost(...)' is therefore inexact: ζ2's W_NEST_SUB_NOT_NESTED must be decided from the entity path / sub shape (index suffix, depth), not from observing Undef. | **PASS** | manual: B18 descendants half: three SheetPart subs equal the explicit list, and the collection sub yields W_NEST_SUB_NOT_NESTED; the code spelling is a working name and is not bound. |
| `nest-sheet-parts-compile-intercept` | producer-self (§4.6) — an intercept for the name nest_sheet_parts in crates/reify-compiler/src/expr.rs beside the cost(...) intercept. The eval-side rewrite beside apply_cost_aggregation (building one PartFootprint per conforming child from scoped extent_*, quantity and material.name cells) is covered behaviourally by B18, not by a grep, because its function and file layout are tactical. | **PASS** | grep `"nest_sheet_parts"` present in `crates/reify-compiler/src` |

## η — #8495

M-WASTE η: integration gate — CI examples (stock pick, nesting) + harness e2e for the §5 rows


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `in-batch-upstream` | producer-upstream — γ, γ2, δ (the sweep, its verdict diagnostics, the GUI paths), ε (stock vocabulary + exact extents), ζ2 (descendants nesting form, and through it ζ). | **PASS** | — |
| `catalog-membership-upstream` | producer-upstream — #8357 (discrete_set sugar; pending), and through it #8355 (the `in` grammar; pending) and #8356 (membership domains in the solver layer; pending). Edge #8357 -> η wired at decompose. Today `constraint t in standard_sheet_thicknesses()` is a grammar-gate rc 1 (3 ERROR nodes at `in`); standard_sheet_thicknesses() already exists as a zero-arg pub fn -> List<Length> in crates/reify-compiler/stdlib/standard_stock.ri. | **PASS** | — |
| `auto-domain-upstream` | producer-upstream — #5470 (AutoParam.domain, is_discrete(), Enum declared domain, Int mining; pending). Edge #5470 -> γ, so -> η. Without it only Bool enumerates and an Enum would narrow. | **PASS** | — |
| `harness-engine-is-a-module-tree` | existing-substrate — crates/reify-eval/tests/harness_engine.rs is a harness ROOT (158 lines) that declares each member as `#[path = "harness_engine/<file>.rs"] mod <file>;` (22 member files, ~10.5k lines under harness_engine/). 'Extending the harness' therefore means a new file under harness_engine/ plus its #[path] line in the root; tests/infra/test_harness_kloc_cap.sh enforces a 20000-line cap per unit and flags an undeclared member file. No new crates/*/tests/*.rs binary (harness-layout ratchet). | **PASS** | — |
| `ci-asserts-example-eval-values-only-through-bespoke-tests` | existing-substrate, answers the brief's question — examples_smoke.rs (crates/reify-compiler/tests/harness_compilation_surface) only asserts every examples/*.ri compiles with stdlib and emits no Error diagnostic; unresolved_function_corpus_sweep.rs and harness_corpus_gates/eval_invariant_corpus_sweep.rs (INV-EVAL-4 / INV-EVAL-5, residual lists STALE_UNDEF_RESIDUALS) sweep examples/ too. NOTHING generic asserts `reify eval` OUTPUT VALUES of an example. Value assertions are bespoke, per example, in two flavours: in-engine tests that read the shipped file from disk (crates/reify-eval/tests/continuous_cost_min_example_e2e.rs; harness_fea_solver_e2e/multi_aspect_objective_example_e2e.rs; harness_auto_resolution/cost_robustness_tradeoff_*) and binary tests that run the real `reify` (crates/reify-cli/tests/harness_cli/cli_affine_tapered_spacer.rs: common::run_subcommand("eval", path) then stdout.contains("TaperedSpacer.det_z = 1.5")). So η's 'examples run under reify eval in CI with the winning values' must be realised by explicit tests that load the two example files. Mismatch to resolve at decompose: the sweep entry is called by cmd_eval (crates/reify-cli), not by Engine::eval, so a harness_engine test must itself call Engine::resolve_geometry_coupled, and the rows whose observable IS the binary (B1 exit codes on `reify eval` and `reify check`, B8 --sweep-budget, B15 kernel-less/solver-less surfaces) can only be asserted from crates/reify-cli/tests/harness_cli.rs. | **PASS** | manual: the test-reference grep below pins that SOME test loads the examples; which harness (engine vs cli) is the open point recorded here. |
| `fixture-registration-guard-live` | existing-substrate — scripts/verify.sh defines _RUST_COUPLED_RI_FIXTURES (a space-delimited basename list consumed by decide_scope) and tests/infra/test_verify_scope.sh PG-DRIFT re-derives the coupled set from git grep over tracked *.rs and goes RED on an unregistered tests/prd-gate/fixtures/<name>.ri reference. No mwaste_*.ri is referenced by any tracked .rs / .sh / .py today (git grep rc 1), so every fixture η's tests read must be added to the list in the same diff. | **PASS** | — |
| `stock-pick-example-file` | producer-self — examples/material_waste_stock_pick.ri (catalog `in` thickness + Enum width class, a volume floor, material + coating Money objective over Stocked blank lets). It enters examples_smoke, unresolved_function_corpus_sweep and the INV-EVAL-4/5 corpus sweep, so it must compile with zero Error diagnostics and leave no stale-Undef residual. | **PASS** | grep `^\s*minimize\b` present in `examples/material_waste_stock_pick.ri` |
| `nesting-example-file` | producer-self — examples/material_waste_nesting.ri (three SheetPart plates sized by one swept catalog thickness in the parent, nest_sheet_parts cost objective, sheet size chosen by the nest). | **PASS** | grep `nest_sheet_parts\(` present in `examples/material_waste_nesting.ri` |
| `examples-are-loaded-by-a-test` | producer-self — at least one test under crates/reify-eval/tests or crates/reify-cli/tests names one of the two example files (the only way the winning values and the exact I_GEOMETRY_SWEEP counts reach CI, see ci-asserts-example-eval-values-only-through-bespoke-tests). | **PASS** | grep `material_waste_(stock_pick\|nesting)\.ri` present in `crates/reify-eval/tests`, `crates/reify-cli/tests` |

## θ1 — #8496

M-WASTE θ1: doc chunks (measurement.md, constraints.md, stock/nesting section), registry-verified, + reify-design index + discoverability


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `eta-upstream` | producer-upstream — η (the documented behaviour has landed and the examples exist). | **PASS** | — |
| `chunk-files-and-fence-gate-exist` | existing-substrate — crates/reify-mcp/src/tools/chunks/measurement.md ('# Measurement & Mass-Property Queries'; its Mass properties paragraph says `bounding_box(geometry) -> BoundingBox` is the axis-aligned extent and says nothing of padding) and constraints.md (sections Inline Constraints, Gating on Interference & Clearance, Constraint Definitions, Optimization, Quantifiers, Constraint Status) exist. Neither mentions Stocked, SheetStock, nesting or sheet stock today. crates/reify-compiler/tests/harness_doc_chunks/fence_gate.rs compiles every reify-fragment fence in every chunk, which is the 'each documented signature compiles' acceptance. | **PASS** | — |
| `pdoccover-fabrication-oracle-scope` | existing-substrate — the PDOCCOVER hard gate (crates/reify-audit/src/pdoccover.rs, baseline crates/reify-audit/pdoccover-baseline.txt) flags any call-shaped name in a chunk (`ident(`) that has no identifier-shaped double-quoted literal under crates/reify-compiler/src/**/*.rs or crates/reify-stdlib/src/**/*.rs, and no declaration in crates/reify-compiler/stdlib/*.ri (tests/ dirs, docs and the chunks themselves are excluded). nest_sheet_parts is covered by ζ2's expr.rs intercept literal; nest_rectangles's registry row lives in crates/reify-builtins/src, which is OUTSIDE the oracle, so a `nest_rectangles(` mention in a chunk needs a literal such as the eval_builtin("nest_rectangles", ..) call in a reify-stdlib in-src unit test, or θ1 will red the gate. Tactical, for ζ and θ1 to satisfy; check with `reify-audit --pattern PDOCCOVER`. | **PASS** | manual: the PDOCCOVER gate itself is the check, run in θ1's verification. |
| `nesting-builtin-documented` | producer-self — the stock / nesting section of a chunk under crates/reify-mcp/src/tools/chunks documents nest_rectangles (and nest_sheet_parts) in intent words, with §4.6 item 7's honesty note. Chunk placement within the directory is left to θ1. | **PASS** | grep `nest_rectangles` present in `crates/reify-mcp/src/tools/chunks` |
| `stocked-vocabulary-documented` | producer-self — measurement.md documents exact extents and the Stocked trait (blank_volume / offcut_volume), scoped to analytic faces (see ε exact-extents-on-torus-and-nurbs). | **PASS** | grep `Stocked` present in `crates/reify-mcp/src/tools/chunks/measurement.md` |
| `reify-design-index-line` | existing-substrate format — .claude/skills/reify-design/SKILL.md 'Probe-verified idioms — index' is a bullet list, one `- **<idiom>**: <advice>. → `<exemplar>.ri`` line per idiom. Ordering problem for the decompose: the docs-truth rule wants the index line to point at the corpus exemplar, which θ2 creates, but θ1 and θ2 are unordered siblings (both after η). Either θ2 owns the SKILL.md line or θ1 -> θ2 is wired. | **PASS** | manual: the line's wording is intent-level and not fixed by the PRD; judged in review against the discoverability signal. |

## θ2 — #8497

M-WASTE θ2: exemplar corpus — geometry-cost exemplar under examples/best_practices + INDEX.md row


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `eta-upstream` | producer-upstream — η. | **PASS** | — |
| `corpus-index-bidirectional-test` | existing-substrate — crates/reify-compiler/tests/harness_compilation_surface/examples_smoke.rs::best_practices_index_matches_corpus_directory requires every *.ri in examples/best_practices to be named in INDEX.md and every bare *.ri filename in INDEX.md to exist; it asserts nothing about prose. examples/best_practices today holds angle_crossings, bolt_circle, clearance_oracle, dimensioned_arguments, discrete_choice, hollow_primitives, negation, symmetry_mirror and INDEX.md. | **PASS** | — |
| `constraint-gate-expected-indeterminate-mechanism` | existing-substrate — crates/reify-eval/tests/harness_corpus_gates/best_practices_constraint_gate.rs (a harness member, not a standalone binary) runs every corpus file through check_source_with_stdlib (SimpleConstraintChecker, no kernel, no solver); `const EXPECTED_INDETERMINATE: &[(&str, u32, &str)]` pins (basename, constraint index, mandatory reason) and is checked both ways (UnexpectedIndeterminate, StaleExpectedIndeterminate). Existing entries: clearance_oracle.ri 0 and 1, discrete_choice.ri 0. θ2's swept and geometric constraints must be listed there with a reason citing the exemplar's own prose. | **PASS** | manual: the exemplar's file name is unspecified in the PRD, so no grep is bound; the gate test, the index test and examples_smoke are the mechanical checks, and a separate reify eval assertion pins the swept value. |
| `refused-continuous-form-stays-out-of-the-corpus` | existing-substrate — an E_GEOMETRY_COUPLED_CONTINUOUS Error would fail examples_smoke's zero-Error rule, and SKIP_SET entries are forbidden for this corpus, so the refused form lives only in the θ1 chunk. Chunk fence rules (crates/reify-compiler/tests/harness_doc_chunks/fence_gate.rs): a bare `reify` fence must compile STANDALONE as a complete module; `reify-invalid` is verified and a CLEAN compile is the violation. The coupled-continuous refusal is an eval-time Error (the module compiles clean), so it must NOT be tagged `reify-invalid`; show it in a `reify` fence that compiles (with the Error described in prose) or in a non-compiled tag (`reify-fragment` / `reify-schematic`). | **PASS** | — |

## θ3 — #8498

M-WASTE θ3: companion corrections to sibling PRDs and the language spec


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `eta-upstream` | producer-upstream — η. | **PASS** | — |
| `stale-fcache-cite-target-does-not-exist` | existing-substrate — no file named cache-input-cone-rekey.md or realization-cache-input-cone-rekey.md exists anywhere under docs/ or plans/ (find, and git ls-files, rc empty). The F-cache PRD that was actually authored is docs/prds/v0_6/selective-realization-eviction.md (#6925). Stale cites verified: continuous-cost-minimisation.md §5 Cross-PRD row 3: `over ranked-solve-result + cache-input-cone-rekey (F-cache`) and §10: `over cache-input-cone-rekey.md (F-cache`); ranked-solve-result.md header (`realization-cache-input-cone-rekey.md (F-cache)`); geometry-algebra-solver-unification.md §10 'gated on the realization-cache input-cone rekey' (no filename). | **PASS** | — |
| `continuous-cost-fcache-cite-removed` | producer-self — continuous-cost-minimisation.md no longer cites the non-existent F-cache document in its §5 row and §10 successor line. Scoped to the stale CONSTRUCT (the filename immediately followed by `(F-cache)`), not the bare filename, so a corrected line that mentions the old name historically does not trip it. Accepted gap: a rewording that keeps `<name> (F-cache)` adjacent would be missed in the other direction (it would still be flagged); manual review of the row covers the replacement text. | **PASS** | grep `cache-input-cone-rekey[^ ]* \(F-cache\)` absent in `docs/prds/v0_6/continuous-cost-minimisation.md` |
| `p3-candidates-unevaluated-variant-documented` | producer-self — docs/prds/v0_6/solution-set-completeness.md (PartialReason block at its §3, with BoxBudgetExhausted and InnerSolveUnproven) gains the additive CandidatesUnevaluated { count } variant and the P3 cross-reference. The variant name is a PRD contract name (§4.3, §7). Absent from that file today. | **PASS** | grep `CandidatesUnevaluated` present in `docs/prds/v0_6/solution-set-completeness.md` |
| `sibling-sections-exist-and-say-the-stale-thing` | existing-substrate — verified present and stale: ranked-solve-result.md (header cluster note naming the F-cache PRD, §0/§6 M-WASTE rows incl. the 'queued — task 4787 dep-wired' row and the 4787 back-edge, §3.1 carrier listing; its §0.1 amendment from the expansion commit is already there, so only the other references remain); discrete-cost-minimisation.md §7 row (`both consume F-result; no direct code seam ... independent`) and §9; geometry-algebra-solver-unification.md §5 Consumers row (M-WASTE 'deferred forward-stub', leaf ο 'inner half') and §10 successor bullet; whole-model-objective-coupling.md §10 — NOTE §8 has no M-WASTE row today, so 'adds the row status' is an addition there; io-lifecycle-bom-cost.md names Discard / DiscardReason only via the io.ri enum (crates/reify-compiler/stdlib/io.ri DiscardReason { Offcut, Scrap, FailedInspection, Waste }) and has no waste-boundary note yet; docs/reify-language-spec.md §10.8 'Geometry-dependent (material / waste) cost' bullet under 'Deferred capabilities' (line ~2482) and feature-table row 18 ('leaving discrete, geometry-dependent, and multi-aspect cost deferred'). The spec §10.8 discrete bullet also says PRD 2 'is queued and not yet authored', which is false (discrete-cost-minimisation.md exists). | **PASS** | manual: docs corrections whose replacement wording is not fixed by the PRD; judged in review against the leaf signal (no sibling PRD or the spec still calls M-WASTE a stub or cites a non-existent F-cache PRD). |

## ι — #8499

M-WASTE ι [MILESTONE]: bookmark — continuous geometry-in-the-loop (the inner layer)


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `p2-geometry-dispatch-upstream` | producer-upstream — #6683 (P2 omicron, the GeometryDispatch trait; pending). It depends on #6251 and on #5436 + #5483 for posed operands (re-pointed from the cancelled #6583 on 2026-09-21; *corrected 2026-10-11* — an earlier version of this row wrongly called that edge stale). Edge #6683 -> ι wired at decompose. | **PASS** | — |
| `sweep-upstream` | producer-upstream — γ (the outer layer whose continuous refusal ι routes from). | **PASS** | — |
| `decision-bookmark-charter` | producer-self — a decision bookmark (execution_class: decision): its deliverable is a /prd design session (arm B on GeometryDispatch, local-optimum honesty, content-addressed realisation memo, native-memory policy, finite-difference budget), never an implementation. No mechanical check. | **PASS** | manual: decision bookmark; dispositioned by expanding into its own PRD (done) or dropping (cancelled). |

## κ — #8500

M-WASTE κ [MILESTONE]: deferred-optimisation decision (§9)


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `eta-upstream` | producer-upstream — η (its examples supply the per-candidate measurements). | **PASS** | — |
| `section-9-trigger-table-exists` | existing-substrate — the PRD's §9 holds the five-row trigger table the decision re-reads (coupled-cone-only realisation, content-addressed realisation memo, warm sweep reuse, parallel candidate evaluation, separable sweeps), each with a justified-when and ruled-out-when condition; #8460 is named as the memo's trigger condition. | **PASS** | manual: decision bookmark: the deliverable is the recorded measurement and either filed follow-ups or a stated reason none is justified. |

## μ — #8501

M-WASTE μ [MILESTONE]: bookmark — multi-instance coupled templates


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `eta-upstream` | producer-upstream — η. | **PASS** | — |
| `instantiation-value-flow-upstream` | producer-upstream — #6592 (instantiation-value-flow, per-instance overlays; pending) and #6657 (solved-value flow; deferred — gating on it is intended). Edges #6592 -> μ and #6657 -> μ wired at decompose. Measured baseline (mwaste_parent_auto_to_sub.ri): a SOLVED parent auto never reaches a sub's ctor-arg overlay (Panel.a.t = undef), a concrete parent param does. | **PASS** | — |
| `decision-bookmark-charter` | producer-self — decision bookmark: lift decision 2's 'instantiated once' limit (per-instance pins, per-instance scoring, how instance objectives combine, separable-vs-product enumeration per §9). Deliverable is a /prd session. | **PASS** | manual: decision bookmark; no mechanical check. |

## ν — #8502

M-WASTE ν [MILESTONE]: bookmark — cross-template sweep sets


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `eta-upstream` | producer-upstream — η. | **PASS** | — |
| `instantiation-value-flow-upstream` | producer-upstream — #6592 and #6657 (see μ); edges wired at decompose. | **PASS** | — |
| `decision-bookmark-charter` | producer-self — decision bookmark: lift decision 2's 'S within T's own autos' and 'one coupled template' limits (child autos joining a parent's sweep set, several coupled templates, composition with M-WHOLE merged clusters). Deliverable is a /prd session. | **PASS** | manual: decision bookmark; no mechanical check. |

## λ — #8503

M-WASTE λ [MILESTONE]: bookmark — true-profile nesting


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `eta-upstream` | producer-upstream — η. | **PASS** | — |
| `decision-bookmark-charter` | producer-self — decision bookmark: irregular profiles (a flat-profile extraction query plus a no-fit-polygon engine), arbitrary rotations, common-line cutting. Deliverable is a /prd session. | **PASS** | manual: decision bookmark; no mechanical check. |

## ω — #8504

M-WASTE ω: close the PRD — terminal Status stamp, AS-AUTHORED freeze header, LIVE/AS-AUTHORED map (PRD + manifest)


| Capability | Evidence / binding | Verdict | Delivered check |
|---|---|---|---|
| `all-leaves-upstream` | producer-upstream — every other leaf: α, β, γ, γ2, δ, ε, ζ, ζ2, η, θ1, θ2, θ3 and the bookmarks ι, κ, λ, μ, ν (a cancelled sibling counts as satisfied, per the overlay). Edges wired at decompose. | **PASS** | — |
| `status-line-is-active-today` | existing-substrate baseline — the PRD's Status line (header blockquote) reads `> **Status:** active — expanded from the 2026-06-24 forward-stub ...`; its first token is `active`, so the PRD is non-terminal until ω stamps it. | **PASS** | — |
| `terminal-status-stamp` | producer-self — the PRD's Status line carries SHIPPED (any case, with or without bold, in either the `**Status:** SHIPPED` or `**Status: SHIPPED.**` ratified form) with the landed leaf ids. The AS-AUTHORED freeze paragraph, the LIVE / AS-AUTHORED map and the matching header on the capability-manifest .md twin are judged in review, not grepped: the word AS-AUTHORED already occurs in the PRD's own §8 ω row, so a grep for it would be vacuous. | **PASS** | grep `^> \*\*Status:?(\*\*)? ?\*{0,2}[Ss][Hh][Ii][Pp][Pp][Ee][Dd]` present in `docs/prds/v0_6/material-waste-cost-minimisation.md` |

## Decompose-time corrections

None of these changes a §3 design decision. Each is recorded in the affected leaf's task description under
"DECOMPOSE-TIME FINDINGS".

1. **B4b's `E_OBJECTIVE_INERT` is not emitted today for a geometry objective.**
   - The code exists (Error severity), but its only emitter is the compile post-pass `inert_objective.rs`, which bails on any call node.
   - α (#8487) emits it engine-side when *S* is empty (producer-self).
   - `mwaste_objective_fallback.ri`'s `reify eval` exit therefore flips 0 → 1.
2. **B10's example is not a realisation failure.**
   - An emptying `difference` realises with volume 0 (rc 0), so it would simply score 0.
   - γ (#8489) uses a zero or negative `box` dimension, or an over-thick `shell`, instead.
3. **`AddOptimal` is exact on analytic faces only — decision 14 is scoped, and B17 holds.**
   - Analytic faces are exact: box (−1.4e-16 relative offcut), pocket (−1.27e-14), sphere, cone, cylinder.
   - A torus (R 10 mm, r 2 mm) keeps zmax +1e-7, at 25.2 ms vs 0.004 ms for `Add`.
   - A NURBS-converted cylinder keeps +1e-7 per side, at 0.447 ms vs 0.019 ms.
   - `Add` overshoots the torus by ~1 mm, so `AddOptimal` is still the right user-visible query.
   - ε (#8492) records these residuals and timings with its §11 Q7 ratio. θ1 (#8496) scopes "exact" and advises a 1 µm stock-fit tolerance for curved-face parts.
   - **Surfaced to Leo.**
4. **The PRD's one-line record shorthand does not parse.**
   - `structure def X { param a : T; param b : U }` (§4.5, §4.6) is a parse error: "syntax error in structure body".
   - A trait `let` needs its body.
   - ε and ζ write one member per line.
5. **The stdlib module name `std.stock` is already taken** by `standard_stock.ri`. ε names the new module distinctly.
6. **γ substrate shapes differ from the PRD's description.**
   - `SolveAllResult` is an enum (`Enumerated{…} | NotEnumerable{…}`).
   - `eval_objective_set` needs `pub` plus a crate-root re-export (and `ObjectiveAbstention` made `pub`).
   - P3's capped → `Partial{BoxBudgetExhausted}` mapping is not landed (`cpsat.rs` still returns `not_attempted()`), so the sweep builds its own `Completeness`.
   - `ENUMERATION_NODE_BUDGET` is a second stop besides the cap.
7. **The warm builders hand-build their AutoParams.** `edit_param` / `edit_source` construct `AutoParam` / `ResolutionProblem` inline over a `HashMap`. α and β wire those two sites.
8. **β's site inventory is larger than the PRD says.**
   - 33 `is_auto()` / `is_auto_cell(` lines.
   - 5 `is_auto_free()` lines.
   - 3 structural `ValueCellKind::Auto { .. }` sites the predicate grep misses.
   - The PRD's "some two dozen" undercounts.
9. **ζ2's limit is the geometry fold, not enumeration.**
   - `enumerate_descendants` flattens collection subs.
   - The fold (`post_process_cross_sub_value_cells`) skips them and handles direct subs only.
   - So `W_NEST_SUB_NOT_NESTED` is decided from the sub's shape or path, never from observing undef.
10. **η's test placement.**
    - Nothing generic asserts example eval values.
    - The sweep entry lives in `cmd_eval`, so CLI-level rows go in `crates/reify-cli/tests/harness_cli.rs`, beside `harness_engine.rs`.
    - Each example also enters `examples_smoke`, `unresolved_function_corpus_sweep` and `eval_invariant_corpus_sweep`.
11. **θ1 / θ2 ordering.**
    - The reify-design `SKILL.md` index line moved from θ1 to θ2 (#8497). The two are unordered siblings, so θ1 could not name θ2's exemplar.
    - PDOCCOVER's oracle does not cover `reify-builtins/src`, so a chunk mention of `nest_rectangles` needs ζ's quoted literal in `reify-stdlib/src`.
12. **θ3 corrections.**
    - `whole-model-objective-coupling.md` §8 has no M-WASTE row, so θ3 adds one.
    - The spec's §10.8 says PRD 2 "is queued and not yet authored", which is false; θ3 fixes it.
13. **One check demoted to `manual`.** `optimality_unproven_finding`'s visibility grep: the `pub(crate)` change is avoidable if the sweep's finding is built inside `engine_eval.rs`, so a grep could stay red after a correct delivery.
14. **bbox consumers the PRD's list missed** are enumerated in ε's description: `process.ri` / `dfm.rs`, `harvest_dfm_let_diagnostics`, `extremal_by_bbox`, and about a dozen tests with padding tolerances.
15. **Out-of-batch follow-up filed: #8510.** A geometry query inside a user `fn` taking a `Solid` never folds after realisation. The PRD measured this and designed around it, but it had no owner.

## G2 walk

| Label | Task | Kind | Consumer |
|---|---|---|---|
| α | #8487 | leaf (B1, B2 pre-γ, B3, B4, B4b, B12, B15, B16) | every eval path; γ |
| β | #8488 | **intermediate** (B14, in-process) | γ, δ |
| γ | #8489 | leaf (B5–B10, B19; B2 swept) | `reify eval` users; δ |
| γ2 | #8490 | leaf (B11) | sweep users |
| δ | #8491 | leaf (B13, GUI harness) | GUI users |
| ε | #8492 | leaf (B17) | `.ri` authors; ζ, η |
| ζ | #8493 | leaf (B18 explicit list) | `.ri` objectives; ζ2, η |
| ζ2 | #8494 | leaf (B18 descendants) | `.ri` objectives; η |
| η | #8495 | integration-gate leaf (§5 end to end) | CI |
| θ1 | #8496 | docs leaf (chunk fence gate, discoverability) | authors, design sessions |
| θ2 | #8497 | docs leaf (exemplar + INDEX + SKILL line) | authors |
| θ3 | #8498 | docs leaf (committed prose) | sibling-PRD readers |
| ι κ μ ν λ | #8499 #8500 #8501 #8502 #8503 | decision bookmarks (born-at-L2 on dispatch) | Leo |
| ω | #8504 | close leaf (terminal header) | PRD readers; PPRDSTATUS |

`grammar_confirmed = true` on every leaf. The `in` grammar is #8355's, reached through the #8357 → η edge.

## G7 walk

Walked against `docs/legibility/design-invariants.md`: no unresolved hit, no waiver.

- **`nothing-vacuous-and-unowned`:** `SheetPart.quantity` and the stock lets are consumed by ζ / ζ2 / η in-batch (staged with a live owner). There is no new declarable surface.
- **INV-SF-1:** UndefCause on every not-resolved auto and on non-`Ok` nest fields.
- **INV-SF-2:** the continuous refusal is Error on every path and must exit nonzero on `reify check` (B1). A finite scope on a kernel-less path is a Warning, so a healthy design never hits Error.
- **INV-SF-3:** W2 gives exactly one outcome per coupled template; `W_NEST_SUB_NOT_NESTED`; `E_OBJECTIVE_INERT` for an empty *S*.
- **INV-SF-4:** the continuous case is an Error, not a permanent Indeterminate. The swept constraints' Indeterminate on kernel-less paths is attributed by `W_GEOMETRY_COUPLED_NOT_RESOLVED{reason}`.
- **INV-SF-5:** `material : String` is a record key matched against `Material.name`; a mismatch is loud (`E_NEST_NO_STOCK`). No stand-in-typed signature.
- **INV-SF-6:** every diagnostic is coded.
- **INV-SF-7:** no grammar.
- **INV-AD:** none; the 0°/90° nesting rotation is internal.
- **INV-PD-1/2:** `NestResult` numeric fields are `Undef` with a cause on non-`Ok`, never a well-formed degraded value.

## D3 verification

`scripts/prd-decompose-verify.mjs` ran from a scratchpad copy: Adversary downgraded to sonnet/high, probes pinned to the fresh debug binary, temp files namespaced.

- **Run `wf_7c7aa944-a2e`** (α, β, γ, ε, ζ/ζ2; 20 agents): α 5/5, γ 4/4 and ζ 6/6 were VERIFIED. β and ε reported BLOCKS, from three harness or authoring artifacts:
  - the Enumerator put β's stderr signature under the key `signature` instead of `stderr_contains`;
  - it bound "eval exits 1" as a `rejection`, which runs `reify check`. That exits 0 on this fixture, the known INV-SF-2 residual owned by #5404 / #7308;
  - for ε it rewrote the `/tmp` fixture paths into `tests/prd-gate/fixtures/` (6 fixture-absent records), and the lead's own ε premise range [0.04000001, 0.04000003] was 10× too tight for the measured 0.0400002 m.
- **Run `wf_eee46968-ebb`** (β, ε re-run with exact match dicts; 8 agents): **PASS** — β 3/3, ε 7/7.

Combined result: every premise is verified with captured output. 0 blocking, 0 malformed, 0 fixture-absent. No substrate premise was falsified.

γ2, δ, η, θ1–θ3, the bookmarks and ω were not submitted to D3. Their premises are producer-upstream edges, GUI-harness behaviour or doc paths, already bound above.

## Dependencies wired (2026-10-11)

46 edges in total: 37 intra-batch and 9 out-of-batch.

Intra-batch edges follow PRD §8; ω depends on all 17 siblings.

Out-of-batch edges and their status when wired:

| Edge | Status |
|---|---|
| #5470 → γ, η | pending |
| #6724 → δ | pending |
| #8357 → η | pending |
| #6683 → ι | pending; it waits on #6251 and #5436 + #5483 (re-pointed from the cancelled #6583 on 2026-09-21) |
| #6592 → μ, ν | pending |
| #6657 → μ, ν | deferred, intended |
