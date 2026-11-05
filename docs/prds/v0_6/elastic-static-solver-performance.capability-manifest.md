# Capability manifest — elastic static solver performance

PRD: `docs/prds/v0_6/elastic-static-solver-performance.md` (landed with this manifest via docs carrier
#8371). Machine-readable twin: `elastic-static-solver-performance.capability-manifest.yaml`
(`schema_version: 1`), hand-stamped 2026-10-08 with the batch's task ids (α #8372 … ω #8387) while the
batch stayed deferred; `commit_planning` re-stamps the same ids at the flip to pending. Both twins are generated from one source, so
they agree row for row. The former unlabelled bookmark (numeric-factor cache) is now leaf π (Q-J),
so every task in the batch has a block.

**Evidence base.** Bound 2026-10-08 against main `5333d5de98` (the PRD read `f93f3b9db9`; every symbol
it cites was re-checked by `git grep` here). Code evidence is cited by symbol, never by line. faer
facts come from the faer 0.24.0 registry source. Prerequisite task records read on 2026-10-08:
#8244, #7080, #7453 and #8252 in full; the statuses of #7079, #6660, #7052, #8251, #8253, #8256,
#8265, #7189, #7452, #7456 and the other §9 neighbours (every one live; #8271 deferred).

**Binaries.** No new solve was run. Probes cited: `target/debug/reify` built 2026-10-06 18:05 in
the main checkout (the PRD's P1 thin-plate probe; M1's own `reify check` of the x-offset two-box
variant and of the `linear_solver` grammar fragment); the lead's μ probe used a debug build of
2026-10-07. Grammar: `tree-sitter parse` from `tree-sitter-reify/` (0 ERROR nodes).

Binding vocabulary:

- **producer-self**: the leaf delivers the capability and its own signal observes it.
- **producer-upstream**: an intra-batch leaf or a named task on a hard edge delivers it.
- **existing-substrate**: present on main today, cited by symbol, verified by `git grep`.

Verdicts: **PASS**; **FAIL** (blocks the batch; failure class named); **OPEN** (a binding awaiting a
ruling; none remain after Leo's Q-H/Q-I/Q-J rulings of 2026-10-08).

## Result (rev 4 — Q-H, Q-I, Q-J ruled and folded, 2026-10-08)

**FAIL count: 0.** **OPEN count: 0.** Leo's three rulings closed the three provisional rows:

- α `predicted-flops-estimate` — Q-I;
- β `deterministic-meaning` — Q-H;
- γ `q-i-work-budget-calibration` — Q-I.

They are now bound normally as PASS.

History: rev 1 raised θ BT23 and ν BT31; rev 2 raised β BT10 (mesh identity). All were resolved by
the drafter's folds.

**Q-J — new leaf π** (numeric-factor cache, C14; depends on β and ζ; λ and ω now depend on π).

- The DAG re-walk over π, λ, ω, C1 `SolveReport.reuse: FactorReuse`, C8's reuse wording, C13 and
  C14 found no inversion. Every capability π needs is produced by β, ζ (and α, ε through them) or
  exists on main.
- Two π-scoped advisories are listed under the DAG table. They concern the fault hook's symbolic
  counter and the eviction cost.

**New mechanical checks in rev 4** (all absent on main today, rc=1):

| Label | Check | Pattern | Path |
|---|---|---|---|
| α | `predicted-flops-estimate` | `\bpredicted_flops\b` | `linear_solve/` |
| β | `warm-state-value-owned-by-adapter` | `\bOpaqueState\b` | `compute_targets/static_linear_solve.rs` |
| γ | `q-i-work-budget-calibration` | `auto_direct_work_budget:\s*Some\(` | `linear_solve/` |
| π | `pattern-hash` | `fn pattern_hash\b` | `linear_solve/` |
| π | `cached-factor-type` | `pub struct CachedFactor\b` | `crates/reify-solver-elastic/src` |
| π | `into-cached` | `fn into_cached\b` | `linear_solve/` |
| π | `prepare-from-cache` | `fn prepare_from_cache\b` | `linear_solve/` |

The rev-2 check β `solve-policy-in-dispatch-context` stands.

**Vacuity run: clean** (full re-run). Every `present` check is absent on main today and every
`absent` check is present. `lint_delivered_checks` raised no reject; its 10 expected
`absent_overbroad` WARNs are listed after the vacuity table.

Per-label counts are under "Verdict counts" below.

## D3 run disposition (2026-10-08)

The premise-verification run covered the six CLI leaves (β, δ, ζ, θ, μ, ν), with executed probes.

- **β, θ, ν: VERIFIED.**
  - The two extent FAILs that rev 1 raised against θ BT23 and ν BT31 were DAG findings, not probe
    vectors. Both rows have been restated and re-walked (above).
  - BT10's new form postdates D3 and was not probed.
- **δ:** blocked only by a Prover fixture-path artefact. The fixture was placed at a tests/ path
  that does not exist. This is not a premise falsification, and no PRD change was made.
- **μ:** fixture-absent (the fixture is μ's own deliverable). The lead closed it by probe:
  `/tmp/prd-gate-fixtures/solverperf_bt30_plate_hole.ri` and `solverperf_bt30_two_boxes.ri` pass
  `reify check` (exit 0) on a debug build of 2026-10-07.
  - The lead's on-disk two-box fixture still offsets along y.
  - M1 re-probed the PRD's x-offset spelling, `translate(box(100mm, 20mm, 20mm), 200mm, 0mm, 0mm)`:
    exit 0.
  - `check` falls back to body-inlining until #6660, so both probes prove spelling, not the solve.
- **ζ: one real finding.** On the dims path the supports list does not change `K`
  (`SYNTHETIC_CLAMP_FACE`). Four support variants gave the same displacement, 5.190735368751905e-4 m,
  and 719 iterations.
  - BT21 was redesigned: it now differentiates case C by `linear_solver`.
  - C10 now names where support grouping is exercised.

## α #8372 — linear_solve seam, Direct tier, policy; SplitCholesky driver shared with explicit Par::Seq; interim Jacobi-PCG tier; zero-RHS rule; linear-solve-fault-injection hook

- `faer-symbolic-exact-factor-size` — existing-substrate — faer 0.24.0 `sparse/linalg/cholesky.rs`: `len_val` is pub on the simplicial and supernodal symbolic structures and on `SymbolicCholesky`; `split_cholesky.rs::SplitCholesky` already reserves the factor from it. Read in the registry source and by git grep on main. **PASS**. Check: none (binding only).
- `faer-numeric-llt-explicit-par` — existing-substrate — `factorize_numeric_llt(…, par, stack, params)` and `factorize_numeric_llt_scratch` take a `Par`; `SplitCholesky` passes `faer::get_global_parallelism()` at three sites today (git grep count 3); the workspace faer is `0.24` with features `std, sparse` only (no rayon). **PASS**. Check: none (binding only).
- `fallible-allocation-pattern` — existing-substrate — `try_reserve_exact` and `MemBuffer::try_new` in `split_cholesky.rs`. **PASS**. Check: none (binding only).
- `faer-symbolic-variant-forcing` — existing-substrate — `CholeskySymbolicParams.supernodal_flop_ratio_threshold` with `SupernodalThreshold::{FORCE_SIMPLICIAL, FORCE_SUPERNODAL}` (faer 0.24.0); simplicial reports `NonPositivePivot { index: k + 1 }`, supernodal `index + s_start`. NOTE: C1's public API exposes no knob for this, so BT4 needs α to expose the forcing to a seam test (e.g. through the fault-injection feature) — α's own design point, not a gap. **PASS**. Check: none (binding only).
- `seam-module` — producer-self (S1) — `reify_solver_elastic::linear_solve`, a module directory. **PASS**. Check: grep `^\s*pub mod linear_solve\b` present in `crates/reify-solver-elastic/src/lib.rs`. Pre-delivery: absent today (rc=1).
- `spd-system` — producer-self (C1) — `SpdSystem::new(k)` checks shape, canonical CSR and a positive stored diagonal, returning `SystemInvalid` (today these are CG panics). **PASS**. Check: grep `pub struct SpdSystem\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `spd-solver-session` — producer-self (S2, C1) — `SpdSolver::prepare` / `solve`, lazy until the first non-zero right-hand side. **PASS**. Check: grep `pub struct SpdSolver\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `typed-solve-stop` — producer-self (S4) — `SolveStop {Cancelled, NotConverged, ResidualAboveTolerance, Failed}`, no filler fields. **PASS**. Check: grep `pub enum SolveStop\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `typed-solve-failure` — producer-self (S4) — `SolveFailure {NotPositiveDefinite, FactorExceedsCeiling, OutOfMemory}`; `WorkerPanicked` arrives with ι. **PASS**. Check: grep `pub enum SolveFailure\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `choose-strategy-pure` — producer-self (C4) — the one home of the tier policy. **PASS**. Check: grep `pub fn choose_strategy\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `linear-solver-choice` — producer-self (C1) — `LinearSolverChoice {Auto, Direct, Iterative}`. **PASS**. Check: grep `pub enum LinearSolverChoice\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `solve-policy` — producer-self (C4) — `SolvePolicy` with `auto_direct_budget` 2 GiB and `direct_ceiling` 8 GiB defaults. **PASS**. Check: grep `pub struct SolvePolicy\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `explicit-par-seq` — producer-self (C2) — every faer call in the shared driver passes `Par::Seq` explicitly. **PASS**. Check: grep `Par::Seq\b` present in `crates/reify-solver-elastic/src/linear_solve`. Pre-delivery: absent today (rc=1).
- `global-parallelism-never-read` — producer-self (C2, BT6) — no code in reify-solver-elastic reads `faer::get_global_parallelism()`. Scoped to a call on a non-comment line (overlay rules 1–2). Accepted gap: an aliased import (`use faer::get_global_parallelism as g`) evades it; the behavioural cover is `tests/split_cholesky.rs` and `tests/eigensolve_shift_contract.rs` green unchanged. **PASS**. Check: grep `^[^/]*get_global_parallelism\(` absent in `crates/reify-solver-elastic/src`. Pre-delivery: present today (rc=0, 3 line(s)).
- `fault-injection-feature` — producer-self (C2) — the `linear-solve-fault-injection` cargo feature, compiled only when enabled and inert unless armed: count reservations and numeric calls, fail a reservation. (The barrier and panic arms are tagged ι in C2.) **PASS**. Check: grep `^linear-solve-fault-injection\s*=` present in `crates/reify-solver-elastic/Cargo.toml`. Pre-delivery: absent today (rc=1).
- `shared-cholesky-driver` — producer-self (C2) — one internal driver (`analyse`, `factorize`) used by both `SplitCholesky` and the Direct tier. **PASS**. Check: manual — the driver is crate-internal and its names are not contract; the behavioural cover is tests/split_cholesky.rs and tests/eigensolve_shift_contract.rs green unchanged (BT6).
- `interim-jacobi-pcg-tier` — producer-self (S3) — the iterative tier is Jacobi-PCG until γ; Auto over budget and `Iterative` resolve to `Pcg(Jacobi)`. **PASS**. Check: manual — BT2 asserts `AutoOverBudget` → `Pcg(Jacobi)`; no variant name is fixed that γ would not also satisfy.
- `zero-rhs-rule` — producer-self (S8) — `‖f‖ = 0` returns `u = 0`, residual 0, converged, with no factorisation, setup or iteration. **PASS**. Check: manual — BT4b — the fault hook counts no numeric call.
- `predicted-flops-estimate` — producer-self (Q-I RULED 2026-10-08; C1, C4) — `FactorEstimate.predicted_flops = Σ_j (c_j + c_j(c_j + 1)/2)`, recomputed from the returned column structure (faer 0.24.0 computes `amd::FlopCount` inside `factorize_symbolic_cholesky` and does not return it; `col_ptr`, `supernode_begin`/`supernode_end`, `col_ptr_for_row_idx` are pub). Its reader is `choose_strategy`'s work term (inactive while `auto_direct_work_budget` is `None`, i.e. until γ) and the strategy Info. **PASS**. Check: grep `\bpredicted_flops\b` present in `crates/reify-solver-elastic/src/linear_solve`. Pre-delivery: absent today (rc=1).
- `bt1-bt6-seam-tests` — producer-self — BT1–BT6 and BT4b against the public seam. BT1's 2,562-DOF beam (61 × 2 × 7 nodes × 3) must be built by α's own test: the production box mesher is inline in reify-eval's `solve_cantilever_fea`, which reify-solver-elastic cannot reach. BT1 asserts properties only, so an equivalent box suffices. **PASS**. Check: manual — BT1–BT6, BT4b — reify-solver-elastic integration tests.

## ε #8373 — pattern-first, node-owned assembly assemble_stiffness; assemble_global_stiffness becomes a wrapper over it

- `assembly-substrate` — existing-substrate — `assembly/global.rs::assemble_global_stiffness` (Deterministic arm: `BTreeMap<(usize, usize), f64>` in element order; Parallel arm: per-thread triplets), `AssemblyMode`, `detect_orphan_dofs`. **PASS**. Check: none (binding only).
- `controlflow-poll-type` — existing-substrate — `core::ops::ControlFlow<()>` (std), shared with α without an α→ε edge (S4). **PASS**. Check: none (binding only).
- `assemble-stiffness-primitive` — producer-self (C12) — `assemble_stiffness(n_nodes, elements, threads, poll)`. **PASS**. Check: grep `pub fn assemble_stiffness\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `btreemap-arm-replaced` — producer-self (C12) — the `BTreeMap` accumulation is gone and `assemble_global_stiffness` delegates to `assemble_stiffness`. NOT mechanised (overlay rule 4): an `absent` grep for `BTreeMap::new()` in `assembly/global.rs` also matches the inline test module's encounter-order reference oracle (a `BTreeMap<(usize, usize), f64>` built from `emit_element_triplets`), the natural test to keep, so it would hold the check red after delivery. The positive `assemble-stiffness-primitive` check plus BT7/BT8 are the cover. **PASS**. Check: manual — rule 4 — the defect construct is also a test-oracle shape in the same file; covered by BT7/BT8 and the positive primitive check.
- `wrapper-keeps-every-caller` — producer-self (C12) — `assemble_global_stiffness(n_nodes, elements, AssemblyMode)` keeps its signature; no caller changes in ε. **PASS**. Check: manual — signature preservation; the cover is the workspace compiling with no caller in the diff.
- `bt7-bt8` — producer-self — BT7 (bitwise across threads ∈ {1, 2, 8}), BT7b (poll breaks on its third call), BT8 (one-off comparison with the pre-ε arm, recorded in the PR). **PASS**. Check: manual — BT7, BT7b seam tests; BT8 is a recorded one-off.

## β #8374 — elastic static on the seam, every lane; ElasticOptions.linear_solver; strategy Info and failure codes; typed cancel; phase-entry progress

- `prereq-8244-cap-and-fixtures` — producer-upstream — #8244 (pending, hard edge): honours `max_iter`/`cg_tolerance` on every solving route, deletes `SOLVER_MAX_ITER`, makes cap exhaustion a coded Error, and ships `fea_cg_cap_exhausted.ri` / `fea_cg_cap_sufficient.ri` (both the reify-eval-fea-tests and reify-cli copies are in its metadata.files). Covers BT13b and the landed `max_iter` default S14 reads. Read on the record 2026-10-08. **PASS**. Check: none (binding only).
- `seam-and-assembly-upstream` — producer-upstream — α (seam, Direct tier, fault hook for BT13d), ε (`assemble_stiffness`). **PASS**. Check: none (binding only).
- `adapter-file` — producer-self (S1) — the solve block moves out of `elastic_static.rs` into the adapter. **PASS**. Check: path `crates/reify-eval/src/compute_targets/static_linear_solve.rs` present. Pre-delivery: absent today (rc=1).
- `stdlib-linear-solver-enum` — producer-self (C6) — `enum LinearSolver { Auto, Direct, Iterative }` in the stdlib. **PASS**. Check: grep `enum LinearSolver *\{` present in `crates/reify-compiler/stdlib`. Pre-delivery: absent today (rc=1).
- `stdlib-linear-solver-param` — producer-self (C6) — `param linear_solver : LinearSolver = LinearSolver.Auto`, appended after `adaptive` (today the last of 17 params). **PASS**. Check: grep `param linear_solver *: *LinearSolver\b` present in `crates/reify-compiler/stdlib/solver_elastic.ri`. Pre-delivery: absent today (rc=1).
- `linear-solver-grammar` — grammar-fixture — `tests/prd-gate/fixtures/elastic_solver_linear_solver_option.ri` (PRD §3; draft copy under the session scratch `draft/tests/prd-gate/fixtures/`, lands with the PRD): `enum LinearSolver { Auto, Direct, Iterative }`, a param of that type, `Opts(linear_solver: LinearSolver.Iterative, max_iter: 50)`. Re-probed on the draft copy: tree-sitter 0 ERROR nodes; `reify check` exit 0 ("All constraints satisfied"). **PASS**. Check: none (binding only).
- `code-FeaLinearSolveStrategy` — producer-self (C8) — the coded Info on every static solve. **PASS**. Check: grep `^    FeaLinearSolveStrategy\b` present in `crates/reify-core/src/diagnostics.rs`. Pre-delivery: absent today (rc=1).
- `code-FeaStiffnessNotPositiveDefinite` — producer-self (C8) — found during the solve (unanchored component, `NotPositiveDefinite`, `NonPositiveDiagonal`); C8 now separates it from the existing `FeaSingularStiffness` (task 2929, pre-solve degenerate elements). **PASS**. Check: grep `^    FeaStiffnessNotPositiveDefinite\b` present in `crates/reify-core/src/diagnostics.rs`. Pre-delivery: absent today (rc=1).
- `code-FeaDirectFactorTooLarge` — producer-self (C8) — explicit Direct over the ceiling. **PASS**. Check: grep `^    FeaDirectFactorTooLarge\b` present in `crates/reify-core/src/diagnostics.rs`. Pre-delivery: absent today (rc=1).
- `code-FeaDirectResidualAboveTolerance` — producer-self (C8) — Direct's residual check failed. **PASS**. Check: grep `^    FeaDirectResidualAboveTolerance\b` present in `crates/reify-core/src/diagnostics.rs`. Pre-delivery: absent today (rc=1).
- `code-FeaSolverOutOfMemory` — producer-self (C8) — a fallible reservation failed. **PASS**. Check: grep `^    FeaSolverOutOfMemory\b` present in `crates/reify-core/src/diagnostics.rs`. Pre-delivery: absent today (rc=1).
- `engine-with-solve-policy` — producer-self (C4) — `Engine::with_solve_policy(SolvePolicy)`; BT13c here, BT14 (γ) and BT26 (ι) consume it. (meshing-service's `with_size_policy` is the pattern, not a dependency — absent on main.) **PASS**. Check: grep `pub fn with_solve_policy\b` present in `crates/reify-eval/src`. Pre-delivery: absent today (rc=1).
- `warm-state-value-owned-by-adapter` — producer-self (C13, new) — the trampoline in `elastic_static.rs` passes the prior `OpaqueState` to the adapter untouched and donates what it returns; the adapter owns the concrete warm-state type and its downcast, so π never edits the hot file. **PASS**. Check: grep `\bOpaqueState\b` present in `crates/reify-eval/src/compute_targets/static_linear_solve.rs`. Pre-delivery: absent today (rc=1).
- `solve-policy-in-dispatch-context` — producer-self (C4, new) — `run_compute_dispatch` (engine_compute.rs, which already calls `install_solve_dispatch_context`) installs the engine's `SolvePolicy` into the solve dispatch context beside the progress sink and `CancellationHandle`, so a trampoline with no engine parameter (the elastic adapter; θ's buckling trampoline) reads it. Anchored on the type appearing in the context's home module. **PASS**. Check: grep `\bSolvePolicy\b` present in `crates/reify-eval/src/solver_progress.rs`. Pre-delivery: absent today (rc=1).
- `unanchored-components` — producer-self (C1) — `boundary::unanchored_components(n_nodes, connectivity, constrained_nodes)`; β's Modules column now lists reify-solver-elastic for it. **PASS**. Check: grep `pub fn unanchored_components\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `cancel-predicate-deleted` — producer-self (S4, BT12) — the `!converged && iterations < max_iter` detection is gone from the hot file (present today, `solve_cantilever_fea`). Narrowed to a non-comment line and tolerant of a `opts.`/`self.` prefix in case #8244 respells it. Accepted gap: a reordered predicate evades it; `solve_cantilever_fea_cancelled_skips_stress_recovery` and BT12 are the cover. **PASS**. Check: grep `^[^/]*!\s*[a-z_.]*converged\s*&&\s*[a-z_.]*iterations\s*<` absent in `crates/reify-eval/src/compute_targets/elastic_static.rs`. Pre-delivery: present today (rc=0, 1 line(s)).
- `execution-modes-gone-from-hot-file` — producer-self (S16, C7) — no `resolve_execution_modes` call and no `PARALLEL_DOF_THRESHOLD` use in `elastic_static.rs` (including its inline tests, which C7 has β delete). `SolverMode::` stays OUT of the pattern: the dual solve's `SolverMode` argument lives until η, and ξ/η own the SolverMode deletions, so including it could re-red this check after β landed and wedge β's dependents. **PASS**. Check: grep `^[^/]*(resolve_execution_modes\(|PARALLEL_DOF_THRESHOLD)` absent in `crates/reify-eval/src/compute_targets/elastic_static.rs`. Pre-delivery: present today (rc=0, 8 line(s)).
- `phase-entry-progress-today-shape` — producer-self (C5) — one `SolverProgressUpdate` per phase entry with `solver_kind` `"direct" | "jacobi-pcg"`, `iter: 0`, `residual: 1.0`; the overlay debounce then starts on a Direct solve (`engineStore.ts::applySolverProgress` debounces on the first event). **PASS**. Check: manual — BT12b; string literals like "direct" are not distinctive enough for a grep.
- `gui-engine-test-updated` — producer-self (C7) — `gui/src-tauri/src/tests/engine_tests.rs`'s `solver_kind == "cg"` / `iter ≥ 1` assertion moves to the phase-entry shape. **PASS**. Check: manual — the reify-gui engine test itself.
- `deterministic-meaning` — producer-self (Q-H RULED 2026-10-08; S6, C6, C13) — `deterministic: true` = bit-identical regardless of path: Direct is path-independent by construction; on the iterative tier the warm start is the only path-dependent input, so the adapter keeps today's `!deterministic && warm_start_beneficial` gate (existing in elastic_static.rs, #4869); `false` (default) keeps warm start; bit-identical reuse (ζ, η, π) is allowed under `true`. Residual in-process hole owned by #7052 (C9). β rewrites the stdlib doc. **PASS**. Check: manual — the gate is existing behaviour moved into the adapter; BT32's `deterministic: true` arm (π) is the reuse cover.
- `stdlib-doc-rewrite` — producer-self (C6, C7) — `threads`, `max_iter`, `cg_tolerance`, `iterations`, `converged`, `linear_solver` docs and the "across machines" text (Q-B). **PASS**. Check: manual — prose; asserting doc wording pins text, not behaviour.
- `options-pin-17-to-18` — producer-self (C6) — `solver_elastic_tests.rs` "exactly 17 param cells" moves to 18 (17 verified on main). **PASS**. Check: manual — a count another landing could move; not a stable dispatch gate.
- `persistent-cache-invalidation` — existing-substrate (S10, awaiting Leo's ratification) — `engine_hash_algo.rs::WORKSPACE_CRATE_COVERAGE` has reify-solver-elastic as `Coverage::Hashed` (src + Cargo.toml) and `engine_hash_closure.txt` lists `faer`, so every Jacobi-era persistent entry misses once β lands. If Leo keeps the decided constant, it is a one-line β addition. **PASS**. Check: none (binding only).
- `bt9-bt13e-cli-and-engine` — producer-self — BT9 (premise probed: thin plate `converged = false`, `iterations = 2000` on `target/debug/reify` of 2026-10-06), BT11–BT13e, BT12b, BT18 (Jacobi). **PASS**. Check: manual — CLI and engine integration tests.
- `bt10-thread-invariance-body` — producer-self over existing substrate (resolved 2026-10-08, rev 3) — BT10 meshes BT30's plate with a hole ONCE in-process (engine-installed gmsh, as BT11) into a `SolverMesh` (≈ 136k DOF) and solves that one mesh through `solve_cantilever_fea` (BT13's provided-mesh shape, existing) at threads ∈ {1, 4, 16}, `deterministic: false`, after first asserting the strategy Info's DOF count > 10,000. Sharing one mesh removes gmsh's run-to-run variation (the plain realization producer still meshes at host parallelism until meshing-service β #8285), so no edge to #8285 is needed. Producers: `threads` → `assemble_stiffness` (ε) wired by β; `Par::Seq` (α); the DOF count in the Info (β). Heavy partition. **PASS**. Check: manual — BT10 engine test.
- `param-drop-declaration-coordination` — producer-self (C6) — if #7080 (pending; depends on #7079, #8244) has landed, β adds `linear_solver` to its C1 declaration; otherwise #7080 does (amendment). **PASS**. Check: manual — cross-task ordering; #7080 owns the declaration.

## ξ #8375 — retire the execution-mode API; move the remaining assembly callers to assemble_stiffness; rebuild tests/determinism.rs

- `upstream-beta` — producer-upstream — β (no hot-file user of the mode API left). **PASS**. Check: none (binding only).
- `solver-mode-parallel-deleted` — producer-self (C7) — the `Parallel { threads }` variant of `SolverMode`. Anchored on the variant declaration line in solver.rs (`SolverMode::Parallel {` match arms start with `SolverMode::` and cannot match). **PASS**. Check: grep `^\s*Parallel\s*\{` absent in `crates/reify-solver-elastic/src/solver.rs`. Pre-delivery: present today (rc=0, 1 line(s)).
- `parallel-helpers-deleted` — producer-self (C7) — `spmv_parallel`, `dot_parallel` and siblings. **PASS**. Check: grep `^[^/]*fn (spmv|dot)_parallel\b` absent in `crates/reify-solver-elastic/src`. Pre-delivery: present today (rc=0, 2 line(s)).
- `threshold-const-deleted` — producer-self (S16) — `PARALLEL_DOF_THRESHOLD` (solver.rs). **PASS**. Check: grep `^[^/]*const PARALLEL_DOF_THRESHOLD\b` absent in `crates`. Pre-delivery: present today (rc=0, 1 line(s)).
- `resolve-execution-modes-deleted` — producer-self (S16) — `resolve_execution_modes` (solver.rs). **PASS**. Check: grep `^[^/]*fn resolve_execution_modes\b` absent in `crates`. Pre-delivery: present today (rc=0, 1 line(s)).
- `assembly-mode-deleted` — producer-self (C12) — `AssemblyMode` (assembly/global.rs). **PASS**. Check: grep `^[^/]*enum AssemblyMode\b` absent in `crates/reify-solver-elastic/src`. Pre-delivery: present today (rc=0, 1 line(s)).
- `assembly-wrapper-deleted` — producer-self (C12) — ε's `assemble_global_stiffness` wrapper. **PASS**. Check: grep `^[^/]*fn assemble_global_stiffness\b` absent in `crates/reify-solver-elastic/src`. Pre-delivery: present today (rc=0, 1 line(s)).
- `morph-caller-moved` — producer-self (C12) — reify-mesh-morph's elasticity calls `assemble_stiffness`. **PASS**. Check: grep `assemble_stiffness\(` present in `crates/reify-mesh-morph/src`. Pre-delivery: absent today (rc=1).
- `modal-caller-moved` — producer-self (C12) — `modal_ops` calls `assemble_stiffness`. **PASS**. Check: grep `assemble_stiffness\(` present in `crates/reify-eval/src/modal_ops.rs`. Pre-delivery: absent today (rc=1).
- `determinism-test-rebuilt` — producer-self (C7) — `tests/determinism.rs` on the seam and `assemble_stiffness`, bitwise across threads ∈ {1, 4, 16}. **PASS**. Check: manual — the rebuilt test is the signal.

## γ #8376 — SA-AMG tier and the shared PCG loop; solve_cg re-expressed on it; NearNullspace; Auto over budget and Iterative become AMG

- `upstream-xi-beta` — producer-upstream — ξ (solver.rs cleaned; transitively β, whose adapter and `Engine::with_solve_policy` BT14 uses). **PASS**. Check: none (binding only).
- `amg-prototype-substrate` — existing-substrate (off main) — the measured SA-AMG prototype on the throwaway branch `task/solver-perf-bench` (`solver_perf_iter.rs` at `3ea4f78320`, an ancestor of the branch tip `de5bbcbc3b`): `amg_setup`, `tentative`, `node_graph`, `aggregate`, `Amg::vcycle`. Verified to exist by git. **PASS**. Check: none (binding only).
- `near-nullspace` — producer-self (S15, C1) — `NearNullspace { dof_node, dof_component, node_coords }`; `SpdSystem::new` gains it. **PASS**. Check: grep `pub struct NearNullspace\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `preconditioner-trait` — producer-self (S11) — one PCG loop generic over `Preconditioner`. **PASS**. Check: grep `pub trait Preconditioner\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `sa-amg-preconditioner` — producer-self (C3) — `SmoothedAggregation` implements it. Anchored on the impl, not the name, so a variant α might declare early cannot satisfy it. **PASS**. Check: grep `impl Preconditioner for SmoothedAggregation\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `solve-cg-on-shared-loop` — producer-self (S11) — `solve_cg` keeps its signature (incl. `mode: SolverMode` until η), panics and zero-RHS return. **PASS**. Check: manual — BT17b — the existing solve_cg callers' tests green unchanged.
- `adapter-builds-near-nullspace` — producer-self — `dof = 3·node + axis` on full systems (verified). **PASS**. Check: manual — BT14/BT15 through the engine.
- `q-i-work-budget-calibration` — producer-self (Q-I RULED 2026-10-08; C4, BT17c) — γ calibrates `auto_direct_work_budget` on R1–R5, T1, B20–B41 so Direct is chosen only where its numeric time ≤ AMG setup + solve, records cases, flops and both times (no timing assertion), and sets it in `SolvePolicy::default()`; `AutoOverWorkBudget` becomes reachable. Anchored on the default turning `Some`, which α (whose default is `None`) cannot satisfy early. **PASS**. Check: grep `auto_direct_work_budget:\s*Some\(` present in `crates/reify-solver-elastic/src/linear_solve`. Pre-delivery: absent today (rc=1).
- `bt14-bt17` — producer-self — BT14 (engine), BT15 (≤ 1e-5, 30× above P5's worst 3.4e-7), BT16, BT17, BT17b, BT18 (AMG). **PASS**. Check: manual — engine and seam tests.

## δ #8377 — max_iter not applicable under Direct (ParamNotApplicable)

- `upstream-beta-option` — producer-upstream — β (`linear_solver`, the adapter, the landed `max_iter` default via #8244 in β's closure). **PASS**. Check: none (binding only).
- `param-not-applicable-code` — producer-upstream — #7079 (param-drop α, pending): mints `DiagnosticCode::ParamNotApplicable` (its manifest's check `ParamNotApplicable` in diagnostics.rs; absent on main today). **PASS**. Check: none (binding only).
- `param-drop-declaration` — producer-upstream — #7080 (param-drop β, pending; depends on #7079 and #8244 — read on the record 2026-10-08): the ElasticOptions C1 declaration with `max_iter` honoured. δ's warning is conditional (Direct only), so `max_iter` stays honoured. **PASS**. Check: none (binding only).
- `adapter-emits-not-applicable` — producer-self (S14) — the adapter emits `ParamNotApplicable` naming `ElasticOptions.max_iter` when a solve resolves to Direct with `max_iter` off its landed default. **PASS**. Check: grep `DiagnosticCode::ParamNotApplicable\b` present in `crates/reify-eval/src/compute_targets/static_linear_solve.rs`. Pre-delivery: absent today (rc=1).
- `bt19-bt20` — producer-self — BT19/BT20 (CLI). NOTE: before γ, BT19's `Iterative` arm (Jacobi, `max_iter: 50`, 5,082 DOF) will exit non-zero by #8244's cap Error; the row asserts only the warning's absence, which holds — do not add an exit-0 assertion to that arm. **PASS**. Check: manual — CLI tests; D3 blocked only by a Prover fixture-path artefact.

## ζ #8378 — solve_load_cases factor reuse

- `upstream-beta` — producer-upstream — β. **PASS**. Check: none (binding only).
- `per-case-options-substrate` — existing-substrate — `LoadCase.options : Option<ElasticOptions> = none` (`stdlib/fea_multi_case.ri`) and `multi_case.rs::solve_multi_case_trampoline`. **PASS**. Check: none (binding only).
- `dims-supports-do-not-reach-k` — existing-substrate (D3 finding) — `SYNTHETIC_CLAMP_FACE` in `elastic_static.rs`: the dims path always clamps the root, so BT21 differentiates case C by `linear_solver`. **PASS**. Check: none (binding only).
- `solver-reuse-scope` — producer-self (C10) — one `SolverReuse` per call, keyed by `(content_hash, LinearSolverChoice)`. **PASS**. Check: grep `struct SolverReuse\b` present in `crates/reify-eval/src`. Pre-delivery: absent today (rc=1).
- `system-content-hash` — producer-self (S9) — `SpdSystem::content_hash`; C1 now marks it as ζ's, so α's landing cannot satisfy this check early. **PASS**. Check: grep `fn content_hash\b` present in `crates/reify-solver-elastic/src/linear_solve`. Pre-delivery: absent today (rc=1).
- `bt21` — producer-self — BT21 (CLI): Direct, "factor reused (case A)", C on the iterative strategy; B bitwise equal to its single-case solve. **PASS**. Check: manual — CLI test; the multi_case.rs header correction is prose.

## η #8381 — DWR dual on the primal's session; solve_dual_cg takes the session; SolverMode deleted

- `upstream-beta-gamma-theta-7453` — producer-upstream — β (new edge: the adapter, and the dual moved onto the adapter's session by whichever of β and #7453 lands second — C10, S16), γ (session on both tiers), θ (no buckling/morph `solve_cg` user), #7453 (goal-oriented γ, pending; deps #7452 done, #7456 pending; amended so that, if it lands after β, it puts the dual on the adapter's session). η edits the adapter only, never the hot file. Both orders are covered: β-second has α's `SpdSolver` upstream; #7453-second finds β's session already landed. **PASS**. Check: none (binding only).
- `solver-mode-deleted` — producer-self (S16) — the single-variant `SolverMode` enum; `solve_cg` loses `mode`. **PASS**. Check: grep `^[^/]*enum SolverMode\b` absent in `crates/reify-solver-elastic/src`. Pre-delivery: present today (rc=0, 1 line(s)).
- `dual-on-primal-session` — producer-self (C10) — η routes the adapter's dual call through a session-taking `solve_dual_cg`. **PASS**. Check: manual — BT22; not mechanised, because the dual call reaches the adapter with β or #7453 (second to land), so a `solve_dual_cg(` grep there could go green before η.
- `bt22` — producer-self — BT22 (engine). **PASS**. Check: manual — engine test.

## θ #8380 — buckling pre-stress (P1, P2) and mesh-morph elasticity on the seam

- `upstream-gamma` — producer-upstream — γ (and ξ through it). **PASS**. Check: none (binding only).
- `buckling-substrate` — existing-substrate — `buckling_kernel.rs::{solve_buckling_kernel, solve_buckling_kernel_p2, build_expansion_map, project_with_expansion}`. **PASS**. Check: none (binding only).
- `morph-substrate` — existing-substrate — `reify-mesh-morph/src/elasticity.rs::{elasticity_morph_with_cg_opts, ElasticityFailure::SolverNotConverged, elasticity_morph_is_deterministic_across_runs_with_same_input}`; cited in `reify-audit/tests/engine_seam_g_allow_cites_live.rs`. **PASS**. Check: none (binding only).
- `buckling-prestress-typed` — producer-self (C11) — `BucklingKernelError::PreStress(SolveStop)` replaces the `assert!`. **PASS**. Check: grep `PreStress\(SolveStop\)` present in `crates/reify-solver-elastic/src/buckling_kernel.rs`. Pre-delivery: absent today (rc=1).
- `buckling-on-seam` — producer-self (C11) — `K_red` becomes an `SpdSystem`. **PASS**. Check: grep `SpdSystem::new\(` present in `crates/reify-solver-elastic/src/buckling_kernel.rs`. Pre-delivery: absent today (rc=1).
- `morph-on-seam` — producer-self (C11) — `elasticity_morph_with_cg_opts` takes a `LinearSolverChoice` and `SolveLimits`. **PASS**. Check: grep `\bLinearSolverChoice\b` present in `crates/reify-mesh-morph/src/elasticity.rs`. Pre-delivery: absent today (rc=1).
- `bt23-buckling-amg-engine-arm` — producer-upstream + producer-self (resolved 2026-10-08) — BT23's CLI arm is Auto-only (Direct; `FeaLinearSolveStrategy` from β). The AMG arm is an engine test that lowers `auto_direct_budget` below `K_red`'s factor via `Engine::with_solve_policy` (β), so Auto resolves to `amg-pcg` (γ) with reason `AutoOverBudget`. The buckling trampoline sees the lowered budget because `run_compute_dispatch` installs the policy in the dispatch context (β, C4) — buckling is a registered ComputeNode trampoline (`"solver::buckling"` in `compute_targets/mod.rs`), so it is reached through `run_compute_dispatch`. `BucklingOptions` gains no `linear_solver`. All producers upstream (θ → γ → ξ → β). **PASS**. Check: manual — BT23 CLI and engine arms.
- `buckling-reads-policy-from-context` — producer-self (C4) — the buckling trampoline reads `SolvePolicy` from the dispatch context; with no context (direct Rust callers) it uses `SolvePolicy::default()`. **PASS**. Check: manual — BT23's engine arm is the behavioural cover; the trampoline file name is not fixed by the PRD.
- `bt24` — producer-self — BT24 (morph tests; `Iterative` with `max_iter = 1` still `SolverNotConverged`; the reify-audit cite updated). **PASS**. Check: manual — reify-mesh-morph and reify-audit tests.

## ι #8382 — bounded-detach cancel (E′) — FactorLedger, detach worker, catch_unwind, Heartbeat, AwaitOrphan, FeaSolverWorkerPanicked

- `upstream-alpha-beta` — producer-upstream — α (fault hook), β (adapter); the per-dispatch carrier exists (`solver_progress.rs::install_solve_dispatch_context` / `current_solve_dispatch_context`). **PASS**. Check: none (binding only).
- `factor-ledger` — producer-self (C5) — one `FactorLedger` per Engine, drop-guarded orphan slot, memory admission (S12). **PASS**. Check: grep `pub struct FactorLedger\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `code-FeaSolverWorkerPanicked` — producer-self (C8). **PASS**. Check: grep `^    FeaSolverWorkerPanicked\b` present in `crates/reify-core/src/diagnostics.rs`. Pre-delivery: absent today (rc=1).
- `worker-catch-unwind` — producer-self (C5) — the worker body runs under `catch_unwind`. **PASS**. Check: grep `catch_unwind` present in `crates/reify-solver-elastic/src/linear_solve`. Pre-delivery: absent today (rc=1).
- `hook-barrier-panic-thread` — producer-self — ι extends α's hook with the barrier and panic arms (tagged ι in C2) and the "records the thread" arm BT26(a) reads, which C2's list omits. **PASS**. Check: manual — BT25–BT27 exercise it.
- `bt25-bt27` — producer-self — BT25, BT26 (a)/(b), BT27 (a)/(b) (engine). **PASS**. Check: manual — engine tests; heartbeat interval decided in ι.

## κ #8383 — GUI phase-shaped solver-progress wire; overlay renders phases and unknown names generically

- `upstream-beta` — producer-upstream — β (phase-entry events). **PASS**. Check: none (binding only).
- `wire-phase-field` — producer-self (C5) — `SolverProgressUpdate` gains `phase: &'static str` and optional `iter`/`residual` (today: `solver_kind`, `iter: u32`, `residual: f64`). **PASS**. Check: grep `pub phase: &\'static str` present in `crates/reify-eval/src/solver_progress.rs`. Pre-delivery: absent today (rc=1).
- `tauri-and-overlay` — producer-self — the Tauri `SolverProgress` struct, `SolverProgressOverlay`, `docs/gui-event-channels/solver-progress.md`. **PASS**. Check: manual — BT29 vitest and the reify-gui engine test.
- `bt28-bt29` — producer-self — BT28 (engine; its `AmgSetup` arm only once γ has landed, stated in the row), BT29 (GUI). **PASS**. Check: manual — engine test and vitest.

## λ #8384 — docs-truth — FEA chunk solver section, fea_solver_choice.ri exemplar and INDEX row, reify-design index line

- `upstream-delta-zeta-theta-iota-kappa-pi` — producer-upstream — δ, ζ, θ, ι, κ, π (closure includes α, β, γ, ε, ξ); π's "reused (cached)" Info is documented here. **PASS**. Check: none (binding only).
- `doc-gates-substrate` — existing-substrate — `harness_compilation_surface/examples_smoke.rs`, `harness_doc_chunks/fence_gate.rs`, `harness_corpus_gates/best_practices_constraint_gate.rs`. **PASS**. Check: none (binding only).
- `exemplar` — producer-self — `examples/best_practices/fea_solver_choice.ri`. **PASS**. Check: path `examples/best_practices/fea_solver_choice.ri` present. Pre-delivery: absent today (rc=1).
- `exemplar-index-row` — producer-self — its `INDEX.md` row. **PASS**. Check: grep `fea_solver_choice` present in `examples/best_practices/INDEX.md`. Pre-delivery: absent today (rc=1).
- `chunk-solver-section` — producer-self — the FEA chunk documents `linear_solver` (`chunks/fea.md` is extended if #8259, #7088 or #8296 created it, else created). Directory-scoped so either creator satisfies it. **PASS**. Check: grep `linear_solver` present in `crates/reify-mcp/src/tools/chunks`. Pre-delivery: absent today (rc=1).
- `reify-design-index-line` — producer-self — one line in the reify-design skill index. **PASS**. Check: grep `fea_solver_choice|linear_solver` present in `.claude/skills/reify-design/SKILL.md`. Pre-delivery: absent today (rc=1).

## μ #8385 — integration gate — a realized plate with a hole from the CLI

- `upstream-beta-gamma-6660` — producer-upstream — β, γ (BT30's Iterative arm), #6660 (pending; gmsh registered in the author binaries — body solves from the CLI; refuses selector BCs, which BT30 does not use). **PASS**. Check: none (binding only).
- `bt30-fixture-spellings` — fixture probe — `reify check` exit 0 on `/tmp/prd-gate-fixtures/solverperf_bt30_plate_hole.ri` (lead, debug build 2026-10-07) and on M1's x-offset variant `m1probe/two_boxes_x.ri` (`translate(box(100mm, 20mm, 20mm), 200mm, 0mm, 0mm)`; `target/debug/reify` of 2026-10-06). NOTE: the lead's on-disk two-box fixture still carries the y offset the PRD rejected. `check` falls back to body-inlining (no trampoline in the CLI until #6660), so this proves spelling, not the solve. **PASS**. Check: none (binding only).
- `unanchored-code-upstream` — producer-upstream — `FeaStiffnessNotPositiveDefinite` (unanchored) from β. **PASS**. Check: none (binding only).
- `two-box-unanchored-fires` — residual — that the union of two separated boxes meshes as two components and that `realized_cantilever_bc_node_sets` clamps only the first (x_min face) is inferred from the code, not executed. **PASS**. Check: manual — μ's implementer confirms on its first CLI run (B1-notes D3 fold).
- `bt30` — producer-self — BT30 (CLI): Auto, Iterative ≤ 100 iterations (2.4× above P3's 42), displacement within 1e-5, the two-box Error. **PASS**. Check: manual — reify-cli test.

## ν #8386 — integration gate — the thin plate on coordinate-target's plate grid

- `upstream-beta-gamma-8252` — producer-upstream — β, γ, #8252 (coordinate-target ζ, pending; depends on #8251 → #7189): `synthetic_grid` gives the dims mesh `y` resolution when a coordinate kind is present. **PASS**. Check: none (binding only).
- `point-supports-static` — producer-upstream (transitive) — `PointSupport` is absent on main (no stdlib or reify-eval hit); #7189 (pending) delivers it, in #8252's closure. **PASS**. Check: none (binding only).
- `fully-restrained-point-supports` — producer-upstream, transitive (resolved 2026-10-08) — BT31 now supports the plate on three FULLY restrained non-collinear `PointSupport`s (kinematically sufficient). Fully restrained point supports are #7189's, in ν's closure via #8252 → #8251 → #7189 (#8252's own fixture uses the same three-point fully restrained shape). No directional restraint (#8256) and no new edge are needed. **PASS**. Check: none (binding only).
- `bt31` — producer-self — BT31 (CLI): converged, Info names strategy and DOFs; #8252's DOF-ceiling note records the solve. **PASS**. Check: manual — reify-cli test.

## π #8379 — numeric-factor cache across evaluations — factor and symbolic analysis in the adapter's warm-state value, keyed by content_hash + choice and pattern_hash; reuse Info; orphan factors never cached

- `upstream-beta-zeta` — producer-upstream — β (the adapter owns the warm-state value, C13; the strategy Info code; `deterministic` gate), ζ (`SpdSystem::content_hash`, `FactorReuse::SessionFactor` precedent). Closure includes α (session API, fault hook) and ε. **PASS**. Check: none (binding only).
- `warm-pool-substrate` — existing-substrate — `warm_pool.rs::WarmStatePool` (`donate_with_cost`, `checkout`, cost-weighted eviction, `drain_events` → `WarmPoolEvent::Evicted { node_id, size_bytes }`, budget from config / `REIFY_WARM_STATE_BUDGET_BYTES`, `DEFAULT_BUDGET_BYTES` 2 GiB, a single item larger than the budget is kept); `Engine::warm_pool_mut` (BT34's budget); the prior `OpaqueState` reaches the trampoline through `run_compute_dispatch` (its own test `run_compute_dispatch_reads_prior_warm_state_from_cache_and_passes_to_trampoline`). **PASS**. Check: none (binding only).
- `pattern-hash` — producer-self (C14) — `SpdSystem::pattern_hash` over CSR row pointers and column indices. **PASS**. Check: grep `fn pattern_hash\b` present in `crates/reify-solver-elastic/src/linear_solve`. Pre-delivery: absent today (rc=1).
- `cached-factor-type` — producer-self (C14) — `CachedFactor` (factor + symbolic + both keys). The `FactorReuse` variants were renamed (`CachedAcrossEvaluations`, `SymbolicAcrossEvaluations`) to avoid a heuristic-1 name clash with this struct. Anchored on `pub struct`. **PASS**. Check: grep `pub struct CachedFactor\b` present in `crates/reify-solver-elastic/src`. Pre-delivery: absent today (rc=1).
- `into-cached` — producer-self (C14) — `SpdSolver::into_cached(self) -> Option<CachedFactor>` (Direct only). **PASS**. Check: grep `fn into_cached\b` present in `crates/reify-solver-elastic/src/linear_solve`. Pre-delivery: absent today (rc=1).
- `prepare-from-cache` — producer-self (C14) — `SpdSolver::prepare_from_cache(system, choice, policy, cached, ctl)` applying the three reuse rules and reporting `FactorReuse::{CachedAcrossEvaluations, SymbolicAcrossEvaluations}`. Anchored on the π-only function, not on the enum variants C1 shows beside α's types. **PASS**. Check: grep `fn prepare_from_cache\b` present in `crates/reify-solver-elastic/src/linear_solve`. Pre-delivery: absent today (rc=1).
- `reuse-under-deterministic` — producer-self (C14, Q-H) — the adapter reads the prior warm state for its factor even when `deterministic` forbids the iterative warm start; both reuses are bitwise a fresh solve. **PASS**. Check: manual — BT32's `deterministic: true` arm.
- `symbolic-call-counting-hook` — producer-self — BT33 asserts "one numeric, zero symbolic calls" through α's fault hook, but C2 lists only reservations and numeric calls. π extends the hook (π's modules include reify-solver-elastic), or C2 adds "symbolic calls (π)". Not a DAG failure. **PASS**. Check: manual — BT33.
- `orphan-factors-never-cached` — by construction (C5, C14) — an orphan's result is dropped on completion and never returns to a live solve, so it can never reach `into_cached`. ι may land before or after π; the invariant holds either way. No BT row asserts it directly. **PASS**. Check: manual — structural; BT25/BT27(b) exercise orphans.
- `pool-memory-accounting` — producer-self (C14) — `estimated_size_bytes = 8n + factor_bytes` counted against the pool budget. ADVISORY: eviction removes the LOWEST `cost_per_byte` first, and the elastic trampoline today sets `cost_per_byte = 1 / size_bytes`, so a large factor would be the first victim; π should derive it from the factorisation's cost (e.g. `predicted_flops` per byte). **PASS**. Check: manual — BT34 (eviction observed through `drain_events`).
- `bt32-bt34` — producer-self — BT32 (load edit → "factor reused (cached)", bitwise a fresh Engine, also under `deterministic: true`), BT33 (material edit → "symbolic analysis reused (cached)", one numeric, zero symbolic), BT34 (budget below both factors → `Evicted`, re-factor, bits equal). **PASS**. Check: manual — reify-eval engine tests.

## ω #8387 — PRD close

- `upstream-all` — producer-upstream — α–ν, ξ and π. **PASS**. Check: none (binding only).
- `terminal-status-header` — producer-self — the PRD's terminal `Status` header per the overlay's freeze shape. **PASS**. Check: manual — prose header; PPRDSTATUS (#6932) detects the leaf/status half.

## Vacuity run (every mechanical check against main today)

Main `5333d5de98`; argv as the gate builds it (`git grep -E -e <pattern> main -- <paths>`, `git ls-tree` for path checks). `present` checks must be absent now; `absent` checks present now.

| Label | Capability | Kind | Expect | rc today | Lines | Verdict |
|---|---|---|---|---|---|---|
| α | `seam-module` | grep | present | 1 | 0 | OK |
| α | `spd-system` | grep | present | 1 | 0 | OK |
| α | `spd-solver-session` | grep | present | 1 | 0 | OK |
| α | `typed-solve-stop` | grep | present | 1 | 0 | OK |
| α | `typed-solve-failure` | grep | present | 1 | 0 | OK |
| α | `choose-strategy-pure` | grep | present | 1 | 0 | OK |
| α | `linear-solver-choice` | grep | present | 1 | 0 | OK |
| α | `solve-policy` | grep | present | 1 | 0 | OK |
| α | `explicit-par-seq` | grep | present | 1 | 0 | OK |
| α | `global-parallelism-never-read` | grep | absent | 0 | 3 | OK |
| α | `fault-injection-feature` | grep | present | 1 | 0 | OK |
| α | `predicted-flops-estimate` | grep | present | 1 | 0 | OK |
| ε | `assemble-stiffness-primitive` | grep | present | 1 | 0 | OK |
| β | `adapter-file` | path | present | 1 | 0 | OK |
| β | `stdlib-linear-solver-enum` | grep | present | 1 | 0 | OK |
| β | `stdlib-linear-solver-param` | grep | present | 1 | 0 | OK |
| β | `code-FeaLinearSolveStrategy` | grep | present | 1 | 0 | OK |
| β | `code-FeaStiffnessNotPositiveDefinite` | grep | present | 1 | 0 | OK |
| β | `code-FeaDirectFactorTooLarge` | grep | present | 1 | 0 | OK |
| β | `code-FeaDirectResidualAboveTolerance` | grep | present | 1 | 0 | OK |
| β | `code-FeaSolverOutOfMemory` | grep | present | 1 | 0 | OK |
| β | `engine-with-solve-policy` | grep | present | 1 | 0 | OK |
| β | `warm-state-value-owned-by-adapter` | grep | present | 1 | 0 | OK |
| β | `solve-policy-in-dispatch-context` | grep | present | 1 | 0 | OK |
| β | `unanchored-components` | grep | present | 1 | 0 | OK |
| β | `cancel-predicate-deleted` | grep | absent | 0 | 1 | OK |
| β | `execution-modes-gone-from-hot-file` | grep | absent | 0 | 8 | OK |
| ξ | `solver-mode-parallel-deleted` | grep | absent | 0 | 1 | OK |
| ξ | `parallel-helpers-deleted` | grep | absent | 0 | 2 | OK |
| ξ | `threshold-const-deleted` | grep | absent | 0 | 1 | OK |
| ξ | `resolve-execution-modes-deleted` | grep | absent | 0 | 1 | OK |
| ξ | `assembly-mode-deleted` | grep | absent | 0 | 1 | OK |
| ξ | `assembly-wrapper-deleted` | grep | absent | 0 | 1 | OK |
| ξ | `morph-caller-moved` | grep | present | 1 | 0 | OK |
| ξ | `modal-caller-moved` | grep | present | 1 | 0 | OK |
| γ | `near-nullspace` | grep | present | 1 | 0 | OK |
| γ | `preconditioner-trait` | grep | present | 1 | 0 | OK |
| γ | `sa-amg-preconditioner` | grep | present | 1 | 0 | OK |
| γ | `q-i-work-budget-calibration` | grep | present | 1 | 0 | OK |
| δ | `adapter-emits-not-applicable` | grep | present | 1 | 0 | OK |
| ζ | `solver-reuse-scope` | grep | present | 1 | 0 | OK |
| ζ | `system-content-hash` | grep | present | 1 | 0 | OK |
| η | `solver-mode-deleted` | grep | absent | 0 | 1 | OK |
| θ | `buckling-prestress-typed` | grep | present | 1 | 0 | OK |
| θ | `buckling-on-seam` | grep | present | 1 | 0 | OK |
| θ | `morph-on-seam` | grep | present | 1 | 0 | OK |
| ι | `factor-ledger` | grep | present | 1 | 0 | OK |
| ι | `code-FeaSolverWorkerPanicked` | grep | present | 1 | 0 | OK |
| ι | `worker-catch-unwind` | grep | present | 1 | 0 | OK |
| κ | `wire-phase-field` | grep | present | 1 | 0 | OK |
| λ | `exemplar` | path | present | 1 | 0 | OK |
| λ | `exemplar-index-row` | grep | present | 1 | 0 | OK |
| λ | `chunk-solver-section` | grep | present | 1 | 0 | OK |
| λ | `reify-design-index-line` | grep | present | 1 | 0 | OK |
| π | `pattern-hash` | grep | present | 1 | 0 | OK |
| π | `cached-factor-type` | grep | present | 1 | 0 | OK |
| π | `into-cached` | grep | present | 1 | 0 | OK |
| π | `prepare-from-cache` | grep | present | 1 | 0 | OK |

`lint_delivered_checks` (no declared files, ref `main`): no polarity reject. 10 `absent_overbroad` WARNs, one per `absent` check; each matches exactly the construct its leaf deletes (and, for β, the hot file's own inline tests of the mode API, which C7 has β delete), and clears once that file is in the leaf's `metadata.files`:

- α `global-parallelism-never-read` — absent_overbroad (warn)
- β `cancel-predicate-deleted` — absent_overbroad (warn)
- β `execution-modes-gone-from-hot-file` — absent_overbroad (warn)
- ξ `solver-mode-parallel-deleted` — absent_overbroad (warn)
- ξ `parallel-helpers-deleted` — absent_overbroad (warn)
- ξ `threshold-const-deleted` — absent_overbroad (warn)
- ξ `resolve-execution-modes-deleted` — absent_overbroad (warn)
- ξ `assembly-mode-deleted` — absent_overbroad (warn)
- ξ `assembly-wrapper-deleted` — absent_overbroad (warn)
- η `solver-mode-deleted` — absent_overbroad (warn)

## Verdict counts

| Label | PASS | OPEN | FAIL | Mechanical checks | Manual |
|---|---|---|---|---|---|
| α | 20 | 0 | 0 | 12 | 4 |
| ε | 6 | 0 | 0 | 1 | 3 |
| β | 26 | 0 | 0 | 14 | 8 |
| ξ | 10 | 0 | 0 | 8 | 1 |
| γ | 9 | 0 | 0 | 4 | 3 |
| δ | 5 | 0 | 0 | 1 | 1 |
| ζ | 6 | 0 | 0 | 2 | 1 |
| η | 4 | 0 | 0 | 1 | 2 |
| θ | 9 | 0 | 0 | 3 | 3 |
| ι | 6 | 0 | 0 | 3 | 2 |
| κ | 4 | 0 | 0 | 1 | 2 |
| λ | 6 | 0 | 0 | 4 | 0 |
| μ | 5 | 0 | 0 | 0 | 2 |
| ν | 4 | 0 | 0 | 0 | 1 |
| π | 11 | 0 | 0 | 4 | 5 |
| ω | 2 | 0 | 0 | 0 | 1 |
| **total** | 133 | 0 | 0 | 58 | 39 |

## DAG-direction walk (every BT row and leaf signal → the producer of each capability it needs)

Edges are those in the PRD's §7 table. "Upstream?" asks whether the producer is in the leaf's
transitive dependency closure.

| Row / signal | Leaf | Capability needed → producer | Upstream? |
|---|---|---|---|
| BT1–BT6, BT4b | α | seam, Direct tier, policy, fault hook → α; faer symbolic forcing → faer 0.24.0 | yes (self, substrate) |
| BT7, BT7b, BT8 | ε | `assemble_stiffness`, `ControlFlow` poll → ε; std | yes |
| BT9 | β | seam → α; assembly → ε; option, Info code → β; today's failure measured (P1) | yes |
| BT10 (rev 3) | β | one shared mesh → meshed once in-process, solved via `solve_cantilever_fea` with a provided `SolverMesh` (existing, BT13's shape); DOF > 10,000 assertion → β's Info; `threads` to assembly → ε+β; `Par::Seq` → α; gmsh in reify-eval tests → existing | yes (rev-2 FAIL resolved; no #8285 edge) |
| BT11 | β | Info on every lane → β; gmsh in reify-eval tests → existing `ensure_gmsh_kernel` dev path | yes |
| BT12, BT12b | β | typed cancel, phase events → β; dispatch-context cancel → existing | yes |
| BT13 | β | `unanchored_components`, NPD code → β (Modules now include reify-solver-elastic) | yes |
| BT13b | β | `fea_cg_cap_exhausted.ri`, cap code → #8244 | yes (hard edge) |
| BT13c | β | `with_solve_policy` → β; ceiling → α | yes |
| BT13d | β | fault hook (fail a reservation) → α; OOM code → β | yes |
| BT13e | β | zero-RHS rule → α | yes |
| BT18 (Jacobi) | β | interim Jacobi tier → α | yes |
| ξ signal | ξ | no hot-file user of the mode API → β; `assemble_stiffness` → ε | yes |
| BT14 | γ | AMG → γ; `with_solve_policy` → β; `AmgSetup` phase events → β's adapter + γ | yes (γ → ξ → β) |
| BT15–BT17, BT17b, BT18 (AMG) | γ | AMG, shared PCG loop → γ; `solve_cg` callers' tests → existing | yes |
| BT17c | γ | `predicted_flops` → α; calibration → γ | yes (Q-I ruled) |
| BT19, BT20 | δ | `linear_solver` → β; `ParamNotApplicable` → #7079; C1 declaration → #7080; landed `max_iter` default → #8244 (in β's closure) | yes |
| BT21 | ζ | per-case options → existing; `content_hash`, `SolverReuse` → ζ; Iterative strategy → α/β | yes |
| BT22 | η | dual on the adapter's session → β or #7453, second to land (η depends on both); session on both tiers → γ; no buckling/morph `solve_cg` user → θ | yes (rev 2: η → β edge added) |
| BT23 CLI arm (Auto) | θ | buckling on the seam → θ; Info code → β | yes |
| BT23 engine arm (rev 2) | θ | `with_solve_policy` → β; policy in the dispatch context → β (C4); trampoline reached via `run_compute_dispatch` → existing (`"solver::buckling"` registered); AMG on `K_red` → γ | yes (rev-1 FAIL resolved) |
| BT24 | θ | morph on the seam → θ; `Iterative` + `max_iter` via `SolveLimits` → α/θ | yes |
| BT25 | ι | barrier arm of the hook → ι (extends α's); adapter → β | yes |
| BT26 | ι | `with_solve_policy` → β; ledger admission → ι; thread-recording hook arm → ι | yes |
| BT27 | ι | panic arm → ι; worker-panicked code → ι | yes |
| BT28 | κ | phase events → β; wire `phase` → κ; `AmgSetup` arm only if γ landed (stated) | yes |
| BT29 | κ | overlay, Tauri struct → κ | yes |
| BT32 | π | reuse Info → β's code + π; `content_hash` → ζ; adapter-owned warm-state value → β (C13); warm-state path and `WarmStatePool` → existing; `deterministic: true` arm → β's gate + π | yes |
| BT33 | π | `pattern_hash`, `prepare_from_cache` → π; one numeric / zero symbolic count → α's hook, symbolic counter added by π (advisory) | yes |
| BT34 | π | `Engine::warm_pool_mut` / budget, `drain_events` → `WarmPoolEvent::Evicted` → existing; re-factor → α/π | yes |
| C1 `SolveReport.reuse` | ζ, η, π | `FactorReuse::SessionFactor` → ζ/η; `CachedAcrossEvaluations`/`SymbolicAcrossEvaluations` → π (π-anchored checks, not the variants) | yes |
| λ signal | λ | documents δ, ζ, θ, ι, κ, π (and β, γ via closure); doc gates → existing | yes |
| BT30 | μ | body solve from the CLI → #6660; Auto → β; Iterative/AMG → γ; unanchored code → β | yes |
| BT31 (rev 2) | ν | plate-capable grid → #8252; three fully restrained `PointSupport`s → #7189 (via #8251) | yes (rev-1 FAIL resolved; no #8256 edge) |
| ω | ω | every leaf incl. π | yes |

No producer-downstream inversion exists among the intra-batch leaves, in either revision. Rev 1's two
extent FAILs (θ BT23, ν BT31) are resolved. Rev 2 raised one, BT10's mesh identity; rev 3
resolves it by meshing once. Rev 4 adds π and finds no inversion. **No FAIL remains.**

## Non-blocking findings — status after the fold

1. **#7453 seam: folded.** Whichever of β and #7453 lands second moves the dual onto the adapter's
   session. η now depends on β and edits only the adapter. β's `absent` check still excludes
   `SolverMode::` (ξ/η own those deletions).
2. **β module scope: folded.** reify-solver-elastic is now listed.
3. **BT10 vacuity: folded.** BT10 now asserts DOF > 10,000, and its mesh-identity FAIL was resolved in rev 3 (one shared mesh).
4. **`content_hash` ownership: folded.** C1 marks it as ζ's.
5. **`FeaSingularStiffness` vs `FeaStiffnessNotPositiveDefinite`: folded** in C8.
6. **BT19's Iterative arm before γ:** unchanged and correct. It asserts only that the warning is
   absent.
7. **Grammar fixture: folded.**
   - `tests/prd-gate/fixtures/elastic_solver_linear_solver_option.ri` is cited in §3.
   - Its draft copy re-probed clean: tree-sitter 0 ERROR nodes, `reify check` exit 0.
   - It must land with the PRD; it is not on main yet.
8. **π: the fault hook's symbolic counter.** BT33 counts symbolic calls, but C2's hook lists only
   reservations and numeric calls. Add "symbolic calls (π)" to C2, or let π extend the hook; π's
   modules allow either. RESOLVED by the lead (2026-10-08): C2's hook now counts symbolic calls too.
9. **π: eviction cost.**
   - `WarmStatePool` evicts the lowest `cost_per_byte` first.
   - The elastic trampoline today sets `cost_per_byte = 1 / size_bytes`, so a large factor would be
     the first victim.
   - π should derive the cost from the factorisation's work (e.g. `predicted_flops` per byte).
     BT34 holds either way. RESOLVED by the lead (2026-10-08): C14 "Eviction cost" sets
     `cost_per_byte = predicted_flops / estimated_size_bytes`.
10. **π: name collision — RESOLVED by the lead (2026-10-08).** The `FactorReuse` variants are
    renamed `CachedAcrossEvaluations` / `SymbolicAcrossEvaluations`; the `CachedFactor` struct keeps its name.
11. **π: orphan factors are never cached by construction.** C5 drops an orphan's result, so it
    cannot reach `into_cached`. No row asserts this directly, and none needs to.
