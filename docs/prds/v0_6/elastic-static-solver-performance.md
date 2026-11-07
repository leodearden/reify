# PRD — Elastic static solver performance: a tiered SPD linear solve

> **Status:** decomposed 2026-10-08 — α #8372, ε #8373, β #8374, ξ #8375, γ #8376, δ #8377, ζ #8378,
> π #8379, θ #8380, η #8381, ι #8382, κ #8383, λ #8384, μ #8385, ν #8386, ω #8387 (PRD close); landed
> with its capability manifest through docs carrier #8371. Authored 2026-10-08 via `/prd` (Leo + Claude,
> agent team) from the solver-performance measurement round of 2026-10-07 (seats A1–A4) and Leo's
> rulings Q-A–Q-J (§4). An adversarial review (C1, 20 findings), a D3 premise run and a capability-
> manifest DAG pass are folded in. Discharges the bookmark #8271.
>
> **Milestone:** v0.6. **Approach:** B + H (FEA is a load-bearing seam; reify-solver-elastic,
> reify-eval, reify-compiler's stdlib, reify-mesh-morph, reify-audit's tests, reify-cli and the GUI
> change). Contract in §5, boundary tests in §6.
>
> **Code anchors** are cited by symbol. Substrate was read on main `f93f3b9db9` (2026-10-08); the
> seam trace A3 was taken on `90b846861f` and re-checked by symbol on `f93f3b9db9`; faer facts are
> from the registry source of faer 0.24.0. The measurements in §3 ran on this host (Ryzen 9 3950X,
> AVX2, no AVX-512; host load 95–390 on 32 threads); their harness is parked on the throwaway branch
> `task/solver-perf-bench` (`solver_perf_direct.rs` at `de5bbcbc3b`, `solver_perf_iter.rs` — which
> holds the SA-AMG prototype — at `3ea4f78320`; never merged).

---

## 1. Goal and consumers (G1)

Every large static elasticity solve reify runs on P1 or P2 tets drops from 10–20 minutes to seconds
on the realized plates measured, and a thin plate converges. A chunky body whose factor sits near the
memory budget would take about a minute to factor (§3 P2), and Q-I's work term (§4) sends it to AMG
instead. The solve becomes a **tiered SPD linear solve**: a sparse direct Cholesky factorisation when
its exact predicted size fits a memory budget, and smoothed-aggregation AMG-preconditioned CG above
it. Every tier is single-threaded and bit-identical across runs and across `threads` values on one
machine. Every static solve says which tier it used and why, every way it can fail is a coded
diagnostic, and a cancel takes effect at once even while a factorisation is running (from ι; §7
names the window before it).

Today a realized 91k–251k DOF plate needs 4.4k–8.3k Jacobi-CG iterations (10–20 min under load), and
the 800 × 500 × 12 mm plate does not converge even on its 5,082-DOF synthetic grid (§3, P1).

| Mechanism | Consumer | Observed by |
|---|---|---|
| The solve seam `linear_solve` — `SpdSystem`, `SpdSolver`, typed outcomes (§5 C1) | `elastic_static` (every lane), `solve_load_cases`, the DWR dual, buckling pre-stress, mesh-morph elasticity | Leaves β, ζ, η, θ |
| Direct tier: faer LLᵀ, explicit `Par::Seq` (C2) | Every solve whose predicted factor fits the budget; the AMG coarse level | Leaves β, μ, ν |
| AMG tier: SA-AMG preconditioned CG (C3) | Solves over the budget; `linear_solver: Iterative` | Leaves γ, θ, μ |
| Strategy policy and budgets, engine-configured (C4) | The seam's `choose_strategy`; the strategy Info | Leaves β, γ |
| `ElasticOptions.linear_solver` and per-tier `max_iter`/`cg_tolerance` semantics (C6) | `.ri` authors; #8244's fixtures; the param-drop declaration (#7080) | Leaves β, δ |
| Pattern-first, thread-count-invariant assembly (C7, C12) | Every adopter's `K`; `threads` | Leaves ε → β |
| Phase progress and bounded-detach cancel (C5) | The GUI solver overlay and its Cancel button | Leaves β (phase-entry updates), ι, κ |
| Cross-case factor reuse (C10) | `solve_load_cases` | Leaf ζ |
| Numeric-factor cache across evaluations (C14) | The GUI edit loop and auto-resolve re-solves (a load-only edit re-solves on the cached factor) | Leaf π |
| Coded strategy Info and failure codes (C8) | CLI and GUI diagnostics; constraints going INDETERMINATE | Leaves β, γ, δ, μ |

**Engine seam (overlay G1 sub-check).** Every consumer is a registered trampoline reached through
engine-integration-norm (`docs/prds/v0_3/engine-integration-norm.md`) §3.4 (ComputeNode dispatch):
`solver::elastic_static`, `solver::multi_case`, `solver::buckling`. Mesh-morph is reached at §3.2
(the VolumeMesh realization edge's morph producer). The seam itself is a library module inside
reify-solver-elastic that these consumers call; it adds no engine seam. The cancel ledger (C5) rides
the per-dispatch context that `run_compute_dispatch` already installs (`install_solve_dispatch_context`),
not a new hook.

## 2. Background: what exists (main `f93f3b9db9`)

**One production static solve.** `elastic_static.rs::solve_cantilever_fea` (the file is 14,463
lines) serves every static P1 route: the dims/synthetic-box overload, the realized-body overload,
heterogeneous and anisotropic materials, both adaptive lanes (`CantileverAdaptiveProblem`,
`RealizedAdaptiveProblem`) and, through `multi_case.rs::solve_multi_case_trampoline`, every case of
`solve_load_cases`. It computes every element stiffness into a `Vec`, assembles `K` with
`assemble_global_stiffness`, builds `f`, applies `apply_dirichlet_row_elimination` (zero row and
column, unit diagonal, lifted RHS — `K` stays symmetric to O(ulp), not bitwise), and calls
`solve_cg_with_warm_state[_progress]` with a hard-coded `CgSolverOptions { tolerance: 1e-6, max_iter:
SOLVER_MAX_ITER = 2000 }`. `ElasticOptions.max_iter`/`cg_tolerance` are never read (#8244, which
lands before this PRD, fixes that and makes an exhausted cap a coded Error).

**The CG.** `solver.rs::solve_cg` and siblings: Jacobi-preconditioned CG, convergence on the
recurrence residual `‖r‖ < tol·‖f‖`; a zero right-hand side returns `u = 0`, `iterations = 0`,
`converged = true` before iterating (`solve_cg_impl`); panics (not `Result`s) on dimension mismatch,
a zero or missing diagonal and `p·Kp ≤ 0`. `SolverMode::{Deterministic, Parallel{threads}}` and
`AssemblyMode::{Deterministic, Parallel{threads}}` are chosen together by `resolve_execution_modes(
deterministic, threads, n_dofs)`: Deterministic iff `deterministic || n_dofs < PARALLEL_DOF_THRESHOLD
(10,000) || threads <= 1`. Parallel is bit-stable only for a fixed thread count. Other `solve_cg`
callers, all Deterministic: `buckling_kernel.rs` (P1 and P2 pre-stress; `assert!` on
non-convergence), `reify-mesh-morph`'s elasticity morph (inhomogeneous Dirichlet, `f = 0`),
`shell_solve.rs`, `membrane_load.rs`, `tensegrity_load.rs`, and `qoi.rs::solve_dual_cg` (no production
caller yet; #7453 adds one).

**Assembly.** `assemble_global_stiffness`'s Deterministic arm accumulates every element block into a
`BTreeMap<(row, col), f64>` in element order and hands faer a duplicate-free triplet list; its
Parallel arm builds per-thread triplet vectors and lets `try_new_from_triplets` sum duplicates.
Callers: `solve_cantilever_fea`, `modal_ops`, `buckling_kernel`, reify-mesh-morph's elasticity,
`shell_solve`, `membrane_load`, `tensegrity_load` and `assembly/volume.rs` — all but the first
hard-code `AssemblyMode::Deterministic`.

**Cancel and progress.** The trampoline's progress closure emits `SolverProgressUpdate { solver_kind:
"cg", iter, residual }` on iteration 1 and every `PROGRESS_STRIDE` (10) iterations and polls the
dispatch context's `CancellationHandle` every iteration. `solve_cantilever_fea` detects a cancel by
the predicate `!converged && iterations < max_iter` and skips stress recovery. The adaptive lanes poll
cancel only (`ambient_cg_cancel_poll`). The GUI maps the update to the `solver-progress` Tauri event
(`docs/gui-event-channels/solver-progress.md`); `engineStore.ts`'s `applySolverProgress` starts a
1 s debounce on the **first** event, after which `SolverProgressOverlay` (iteration, residual, a
Cancel button) becomes visible — a solve that emits no event shows no overlay and no Cancel.

**Warm start.** `warm_state.rs::CgWarmState` (the iterate `u`) is donated to the engine's
`WarmStatePool`; the trampoline uses it iff `!deterministic && warm_start_beneficial(...)` (#4869).
Every case of `solve_load_cases`, both adaptive lanes and every non-static solver run cold.

**Direct factorisation exists for modal.** `split_cholesky.rs::SplitCholesky` drives faer's
`factorize_symbolic_cholesky` + `factorize_numeric_llt` on the lower triangle, reserves the factor
with `try_reserve_exact` and the scratch with `MemBuffer::try_new` (so out-of-memory is an error, not
an abort), and reads the thread count from `faer::get_global_parallelism()` at three sites. The
workspace's faer (`0.24`, features `std, sparse`) has no `rayon` feature, so that global is `Par::Seq`
today — by feature unification, not by intent.

**Cache identity.** Two keys matter. The **persistent** FEA key (`Engine::persistent_cache_key`)
combines the node's structural key with a hash of every evaluated argument value, so an
`ElasticOptions` literal at a direct dispatch already reaches it — except on the body overload's
redispatch path, which passes a constant `ContentHash(0)` (#7052). The **in-process** structural key
(`compute_cache_key`, the NodeCache) carries `options_hash = ContentHash(0)` because the options value
is not lowered into the node's value inputs (#7052 makes it present). The module doc of
`elastic_static.rs` ("not part of the FEA cache key") describes only the latter. The persistent cache
is also invalidated wholesale by `ENGINE_VERSION_HASH`, which `engine_hash_algo.rs::
engine_version_hash_for` composes from the sources of every `WORKSPACE_CRATE_COVERAGE` row marked
`Hashed` — reify-solver-elastic's and reify-eval's `src/` among them — plus a pinned `Cargo.lock`
closure (`engine_hash_closure.txt`) that includes `faer`.

**No memory guard.** `ElasticOptions.max_dofs` is an adaptive stop rule checked after a solve, and
meshing-service θ (#8291) bounds tet count, not factor size. Nothing bounds Cholesky fill.

**What the v0.3 PRD deferred.** `docs/prds/v0_3/structural-analysis-fea.md` item 12: "AMG
preconditioner deferred — Jacobi is enough for v0.3 first-cut." This PRD resolves that deferral.

## 3. Substrate verification (G3) and measurements

| Assumed capability | Verdict | Evidence |
|---|---|---|
| faer sparse symbolic Cholesky giving the exact factor size before numeric allocation | **Exists** | `SymbolicCholesky::len_val` (padded supernodal storage on the supernodal path); `SplitCholesky::try_new_col_major` reserves from it |
| faer numeric LLᵀ with an explicit `Par` argument | **Exists** | `factorize_numeric_llt(…, par, stack, params)` and `factorize_numeric_llt_scratch(par, params)`; `SplitCholesky` passes the global today |
| A non-positive pivot reported with its index | **Exists, with a per-variant base** | `factorize_numeric_llt` returns the dense `linalg::cholesky::llt::factor::LltError` (one variant, `NonPositivePivot { index }`). The simplicial path reports `index: k + 1` (1-based), the supernodal path `index + s_start` (0-based); both are positions in the permuted order. faer picks supernodal when flops / nnz(L) exceeds `supernodal_flop_ratio_threshold` × `CHOLESKY_SUPERNODAL_RATIO_FACTOR`, so small systems are simplicial |
| A deterministic work estimate from the symbolic structure | **Partial** — faer computes `amd::FlopCount` (`n_div + n_mult_subs_ldl`) inside `factorize_symbolic_cholesky` and does not return it; the column structure it returns (`col_ptr` on the simplicial path; `supernode_begin`/`supernode_end`/`col_ptr_for_row_idx` on the supernodal) is enough to recompute it | faer `sparse/linalg/cholesky.rs` |
| Fallible allocation for factor and scratch | **Exists** (pattern) | `try_reserve_exact`, `MemBuffer::try_new` in `SplitCholesky` |
| SA-AMG for 3D elasticity with a rigid-body near-nullspace | **Exists as a prototype**, not in the workspace | `solver_perf_iter.rs` (`amg_setup`, `tentative`, `node_graph`, `aggregate`, `Amg::vcycle`) on `task/solver-perf-bench`; it hard-codes `dof = 3·node + axis` |
| Node and component of every DOF at every adopter | **Exists** | full systems: `dof = 3·node + axis`; buckling's `K_red`: `build_expansion_map` enumerates the surviving full DOFs `g = 3·node + axis` in order, each with weight 1 (production buckling passes no MPCs) |
| A per-dispatch carrier for cancel and progress | **Exists** | `solver_progress.rs::install_solve_dispatch_context` / `current_solve_dispatch_context` |
| An enum-typed option in `ElasticOptions` | **Exists** (pattern) | `ElementOrder`, `ShellForce`; the committed fixture `tests/prd-gate/fixtures/elastic_solver_linear_solver_option.ri` (the `LinearSolver` enum, a defaulted `linear_solver` param and a by-name `linear_solver: LinearSolver.Iterative, max_iter: 50` construction, on a local stand-in for `ElasticOptions`) parses with 0 ERROR nodes and `reify check` exits 0 (2026-10-08; landed with this PRD) |
| `ParamNotApplicable` diagnostic and the C1 declaration | **ABSENT** — #7079 / #7080 | δ depends on both |
| Body solves from the CLI | **ABSENT** — #6660 | μ depends on it |
| A plate-capable synthetic grid | **ABSENT** — coordinate-target ζ #8252 | ν depends on it |
| Persistent-cache invalidation on a solver change | **Exists** | `ENGINE_VERSION_HASH` (§2) |

**Measurements (2026-10-07; harness and logs as in the header).** Realized plate-with-hole meshes
R1–R5 (91,146–250,860 DOF, P1), a thin plate T1 (800 × 500 × 12 mm on a 120 × 75 × 6 grid, 193,116
DOF), structured cubes B20–B60 (24k–648k DOF). CPU seconds are single-thread utime + stime; wall is
inflated by load and is quoted only where nothing better exists. "Factor size" below is faer's
`len_val` — what the harness recorded and what the policy reads.

- **P1 — today.** Production Jacobi-CG (`solve_cg` Deterministic, tol 1e-6): R1 4,364 iterations,
  165 s wall / 127 s CPU; R2 6,775; R3 5,382; R4 8,319; R5 5,949 (dump `meta.json`). Hand-rolled
  scalar Jacobi on T1 needs 20,873–21,170. `SolverMode::Parallel{16}` is slower in wall than
  Deterministic at every size measured (R1 497 s vs 165 s) and burns 1.3–2.7× the CPU. **Probe
  2026-10-08:** `reify eval` (debug build of 2026-10-06) on the dims overload, 800 × 500 × 12 mm,
  `ShellForce.Off`, `deterministic: true` — the 5,082-DOF synthetic grid prints `converged = false`,
  `iterations = 2000` (beside `convergence_status: Converged`, #8266's defect).
- **P2 — direct** (faer 0.24 supernodal, AMD, `Par::Seq`): numeric factorisation R1 2.1 s CPU, R1–R4
  3.0–11.6 s wall, R5 44.7 s, T1 38.4 s; symbolic 1.2–2.0 s; `len_val` 39M–217M (0.3–1.6 GiB); a
  further right-hand side costs 0.05–0.51 s; relative residual 2e-12–1e-11 on every plate. Fill is the
  limit: `len_val` ∝ DOF^1.49 on plates, DOF^1.545 on cubes; numeric CPU ∝ DOF^2.05 / DOF^2.31. The
  81k-DOF cube B30 (1.0 GiB) factors in 20.5 s against AMG's 2.3 s; the 207k cube B41 needs 520M
  (3.9 GiB) and 429 s; the 398k cube 914 s CPU and 11.6 GB. Extrapolating the cube fit to the 2 GiB
  budget gives ≈ 60 s CPU before load. Factor cost depends on mesh structure, not only DOF (R5 vs R4
  at equal DOF: 217M vs 121M).
- **P3 — AMG** (prototype SA-AMG, rigid-body near-nullspace, strength θ = 0.05, symmetric
  Gauss–Seidel V(1,1), coarse level ≤ 4,000 DOF factored directly): 20–42 iterations and 5.0–20.4 s
  CPU on R1–R5 and T1; 13–16 iterations on every cube up to 398k DOF (B400k 14.1 s CPU, 2.1 GB vs
  direct 914 s / 11.6 GB). θ = 0 needs 39–211 iterations; θ = 0.25 stalled coarsening on R2 (a
  106k-DOF coarse level, 5.7 GB). Clamped (isolated) nodes never aggregate, so the first version
  stalled at ≈ 4,800 DOF on B200k; a stall guard (a level shrinking by < 10 % becomes the coarsest)
  fixed it without changing any earlier result. **Direct vs AMG:** above ≈ 1 GiB of factor, AMG was
  faster on every measured case (R5 44.7 s vs 17.3 s; T1 38.4 s vs 16.2–20.4 s; B30 20.5 s vs
  2.3 s); below it on the plates Direct was 1–2× faster (R1–R4 2.6–10.4 s vs 5.0–20.2 s CPU).
- **P4 — rejected candidates.** IC(0): 3.5–5.5× fewer iterations than Jacobi, only 1.2–1.8× less CPU.
  3 × 3 node-block Jacobi: 4–5 % fewer iterations, no CPU gain. Geometric nested dissection: −30 %
  fill on structured grids, 2–5 % on gmsh plates; node-graph AMD equals DOF AMD within 1–2 %. Forced
  simplicial Cholesky: 19× the CPU.
- **P5 — accuracy.** At tol 1e-6 every method's displacement is within 5e-9–3.4e-7 (relative 2-norm)
  of the Cholesky reference; 1e-8 buys 3e-10–5e-9. Loose tolerances are unsafe for Jacobi/IC(0)
  (error 0.25–0.6 at 1e-3). On the bending-loaded thin plate (T1z) no iterative method can push the
  **true** f64 residual below ≈ 1e-7–1.6e-6, although the recurrence residual reaches 1e-8.
- **P6 — assembly.** `assemble_global_stiffness` (the triplet arm, `Parallel{16}` in the harness) took
  6–28 s at 91–250k DOF, 156 s and a 16 GB peak at 1.6M DOF — at least the factorisation's cost on
  every plate once the direct tier is in. Pattern-first assembly is **unmeasured**; ε records it.
- **P7 — determinism.** Every sequential method and `Par::Seq` faer: bit-identical across repeats and
  processes. faer under rayon: a different bit pattern per thread count. Across machines faer is not
  pinnable: `private-gemm-x86` dispatches AVX-512 vs AVX2 and GEMM blocking from cpuid at run time.

**G6 premise status.** Speed: established (P1–P3). Memory: established per family (P2, P3). Direct
accuracy: established (P2). AMG iteration counts: established on the measured families only; a new
family's count is recorded by the leaf that first runs it, never asserted from these numbers alone.
Assembly speed after ε: unmeasured (P6).

## 4. Resolved design decisions

**Ruled by Leo, 2026-10-07/08** (restated, not relitigated):

| # | Decision |
|---|---|
| **Q-A** | **Both tiers in this PRD.** Direct (faer Cholesky) when the symbolic analysis — the exact factor size, known before numeric allocation — is within budget; SA-AMG-preconditioned CG above it. Jacobi-CG stays only for the small non-elasticity solvers (shell, membrane, tensegrity). |
| **Q-B** | **Every tier is single-threaded and bit-identical across runs and across `threads` values on one machine.** Across machines results are tolerance-equivalent, not bit-identical (P7). `deterministic`/`threads` then govern only assembly and gmsh. The "across machines" text in the stdlib `ElasticOptions.deterministic` doc and in `solver.rs` is rewritten. No test pins cross-machine bits. |
| **Q-C** | **Engine-level policy, no `.ri` knob.** Auto picks Direct iff the predicted factor ≤ 2 GiB (`len_val` ≲ 2.7e8), else AMG. This is a **memory** budget: on chunky bodies AMG is faster well under it (P3), which Q-I addresses. An explicit Direct request may go to a hard 8 GiB ceiling and fails with a coded Error beyond it. |
| **Q-D** | **`ElasticOptions.linear_solver : LinearSolver = Auto`**, variants `Auto \| Direct \| Iterative`, and a coded Info on every static solve naming the strategy and why. Under Direct, `cg_tolerance` is the post-solve relative-residual check and `max_iter` is not applicable (the param-drop `not_applicable` class). `max_iter`/`cg_tolerance` (#8244) govern the iterative tier. |
| **Q-E** | **Bounded detach (E′).** Cooperative cancel everywhere the code is ours; only faer's symbolic and numeric factorisation run on a worker thread that owns its inputs; on cancel the solve returns `Cancelled` at once and the worker becomes an orphan whose result is dropped. An Engine-owned ledger caps orphans at one and accounts their predicted bytes; **admission waits for memory** and never switches strategy; `catch_unwind` and fallible allocation on the worker; a debug event when an orphan completes. (Confirmed 2026-10-08.) |
| **Q-F** | **Assembly in scope**: pattern-first CSR assembly, no triplet buffer (P6). |
| **Q-G** | An adversarial critic seat reviews this draft before it returns to Leo. |
| **Q-H** | **`deterministic: true` means "always the same answer (bit-identical), regardless of the path taken to get there"**; speed matters, so warm starts matter. The Direct tier is path-independent by construction, flag or not. On the iterative tier warm start is the only path-dependent input, so `deterministic: true` disables it and `deterministic: false` (the default) keeps it for speed. Any reuse bit-identical to a fresh solve — the same factor or hierarchy on the same RHS, a symbolic analysis reused for an identical pattern — is allowed under `deterministic: true`. Residual hole, owned by #7052: the in-process NodeCache key carries no options, so a `deterministic: true` request can still be served an earlier warm-started iterative result in-process until #7052 lowers options into that key (C9). (Ruled 2026-10-08.) |
| **Q-J** | **The numeric-factor cache is in scope** as leaf π (C14), not a bookmark. (Ruled 2026-10-08.) |
| **Q-I** | **The Auto policy gains a predicted-work term**: Direct iff `factor_bytes ≤ auto_direct_budget` **and** the predicted factorisation flops, computed deterministically from the symbolic column structure, are ≤ `auto_direct_work_budget`. γ calibrates the work budget from the measured families so that Direct is picked only where its numeric time is at most AMG's setup + solve. Engine-level, no `.ri` knob (C4). (Ruled 2026-10-08.) |
| **D-1** | AMD ordering (faer's default). No iterative refinement. IC(0) and node-block Jacobi rejected (P4). |
| **D-2** | `SolverMode::Parallel` leaves the static path and its helpers are deleted. |
| **D-3** | Warm start is kept for the AMG tier. A numeric-factor cache for load-only GUI edits was first a bookmark; Q-J promotes it to leaf π (C14). |
| **D-4** | Adopters: elastic static (every lane), `solve_load_cases` factor reuse, the DWR dual (after #7453), buckling pre-stress (P1 and P2), mesh-morph elasticity. Shell, membrane and tensegrity stay on `solve_cg`: small systems, and tangent-K definiteness is unproven there, so Direct would turn silent paths into errors. |
| **D-5** | Persistent cached CG results are invalidated; `linear_solver` joins the options hash once #7052 lowers options into it. (A solver pipeline version in `compose_engine_version_hash` was also decided — see S10.) |
| **D-6** | #2952 is amended (its premise is CG iteration counts). |
| **D-7** | Timing claims are recorded benchmark or CLI re-runs, never wall-clock assertions in tests. |
| **D-8** | Gates on realized bodies depend on #6660; the thin-plate gate on coordinate-target ζ #8252. |

**Decided this session** (design within the rulings):

| # | Decision | Why |
|---|---|---|
| **S1** | **The seam is a module directory, `reify-solver-elastic/src/linear_solve/`**, one file per concern (system, policy, direct, amg, pcg, detach, outcome). `elastic_static.rs` gains no solver code: β moves its solve block into a new adapter file, `compute_targets/static_linear_solve.rs`, and the file shrinks. β is the **only** leaf that edits `elastic_static.rs`; every later leaf works in the adapter. | Heuristic 14 (no file too large) and 13 (files make sense alone); one hot-file leaf. Keeping the seam inside reify-solver-elastic keeps it under `ENGINE_VERSION_HASH` coverage with no new `WORKSPACE_CRATE_COVERAGE` row. |
| **S2** | **A solver is a session**: `SpdSolver::prepare` does the analysis and the factorisation or the AMG setup once; `solve` takes one right-hand side. | One factor or hierarchy serves every RHS (multi-case, DWR dual) — heuristic 5 scopes the expensive state to its consumer. |
| **S3** | **The iterative tier exists from α as Jacobi-PCG and becomes AMG at γ.** | β can then adopt the seam before γ with no regression over budget (Auto over budget = today's Jacobi), and no variant is declared before it does something (the umbrella invariant). |
| **S4** | **Outcomes are typed** (`Result<Solution, SolveStop>`): cancel, non-convergence, Direct's residual failure, a non-SPD pivot, the explicit-Direct ceiling, out-of-memory and a worker panic are distinct variants, none with filler fields. The `!converged && iterations < max_iter` predicate is deleted. One cancel-poll type everywhere: `core::ops::ControlFlow<()>`. | Heuristics 12 and 10; heuristic 11 for the poll type (std's, so ε and α share it without depending on each other). |
| **S5** | **No fallback between tiers on failure.** A non-SPD pivot, out-of-memory or an explicit-Direct ceiling breach is a coded Error, never a silent retry on the other tier. | A memory- or failure-dependent tier would make bits depend on the machine's state (Q-B); `no-silent-fail-soft`. |
| **S6** | **Assembly is node-owned and bit-identical at every thread count, and equal to today's Deterministic arm.** Each thread owns whole rows and sums each entry over the incident elements in element order — the order the `BTreeMap` arm uses today. `threads` sets assembly parallelism and never changes a bit. **`deterministic` is Q-H's path-independence**: it disables warm start, the only path-dependent input. Under Direct the intent is already satisfied by construction, so `deterministic: true` there raises no warning — unlike `max_iter`, whose intent cannot apply (S14). Bit-identical reuse (ζ, η, π's factor cache) is allowed under it. | Q-B's "bit-identical across `threads` values" cannot hold while `AssemblyMode::Parallel` sums in thread order; this keeps `threads` and `deterministic` honoured (INV-PD-1) rather than inert. gmsh already runs on one thread (meshing-service D-J), so Q-B's "gmsh" half has no remaining referent. |
| **S7** | **Auto always runs the symbolic analysis**; the decision reads only the symbolic structure (`len_val` and Q-I's `predicted_flops`), the DOF count and the policy constants. | Exact and deterministic (no timing); costs 0.1–2 s on the plates, 14 s at 1.6M DOF (P2). |
| **S8** | **Direct reports `iterations = 0` and `converged = true` only after its residual check passes**; a failed check is `SolveStop::ResidualAboveTolerance`, not a result. **A zero right-hand side** (`‖f‖ = 0`) on every tier returns `u = 0`, `true_rel_residual = 0`, converged, with no factorisation, setup or iteration. | A direct solve performs zero Krylov iterations — a true count, not a sentinel (INV-PD-2). The zero-RHS rule keeps today's `solve_cg` behaviour and avoids a NaN residual (`0/0`) on load-free cases and morph ticks. |
| **S9** | **Cross-case reuse is keyed by the post-BC `K` itself** (a content hash of its pattern and value bits) plus the solve choice, not by a semantic signature. | Reuse is sound by construction (heuristic 10); cases differing in supports, material or mesh fall out naturally. The cost is one assembly per case; pattern-first assembly time is unmeasured (P6), and ζ records it. |
| **S10** | **No separate solver-version constant.** This **departs from the DECIDED list** ("a solver pipeline version joins `compose_engine_version_hash`"). It is verified redundant: `WORKSPACE_CRATE_COVERAGE`'s reify-solver-elastic row is `Hashed` and `engine_hash_closure.txt` pins `faer`, so every persisted Jacobi result misses once β lands. **Ratified: listed to Leo as "decided unless you object" on 2026-10-08; no objection.** | One home for "the engine changed" (heuristic 11). The policy constants live in hashed source too. If Leo prefers belt-and-braces, it is a one-line addition in β. |
| **S11** | **One PCG loop.** `linear_solve::pcg` is generic over a `Preconditioner` trait with two implementations, `Jacobi` and `SmoothedAggregation`; γ re-expresses `solve_cg`'s body as that loop with `Jacobi`, keeping `solve_cg`'s signature, panics, zero-RHS early return and `CgIterationControl` contract. | Heuristic 11 (one CG) and 3 (the preconditioner is one axis). Shell, membrane and tensegrity stay on `solve_cg` (D-4) and change only at round-off; none of their tests pins a value tighter than 1e-9 against a constant or an exact CG count. |
| **S12** | **Orphan admission is by memory (Q-E).** A factorisation that finds an orphan in flight proceeds alongside it iff the orphan's predicted bytes plus its own ≤ `direct_ceiling`, **and then runs undetached** (inline, cancel at phase boundaries); otherwise it waits (`AwaitOrphan`, cancellable) until the orphan finishes. Hence there is never more than one orphan. The strategy never reads ledger state. | Leo's reading of E′, made concrete; memory stays bounded by the ceiling; bits never depend on timing (Q-B). |
| **S13** | **The strategy Info's text is deterministic**: strategy, reason, DOF count, predicted factor bytes, budget, iterations, true residual. Timings are not in the Info or the report; κ derives elapsed time per phase from event arrival. | The Info replays from the persistent cache (#7245) and appears in CLI goldens. |
| **S14** | **`max_iter`'s not-applicable warning fires whenever a solve resolves to Direct with `max_iter` off its landed default**, whether Direct was requested or chosen by Auto. `max_iter` stays in the honoured set (the iterative tier reads it). "Default" is read from #8244's landed stdlib, not assumed. | Q-D literally; the author learns why their cap had no effect. |
| **S15** | **The near-nullspace is carried per DOF as `(node, component)` plus node coordinates**, and the seam builds the six rigid-body columns itself. Buckling's reduced DOFs carry the `(node, component)` of the full DOF each one enumerates. | One home for the rigid-body modes (heuristic 11); the per-DOF node also defines the AMG block partition, which a bare six-column `B` would not; it covers full P1/P2 systems and the reduced buckling system uniformly. |
| **S16** | **`SolverMode` shrinks then goes.** β removes every `elastic_static.rs` use of `resolve_execution_modes`/`SolverMode`/`PARALLEL_DOF_THRESHOLD`; ξ deletes `Parallel`, its helpers, `PARALLEL_DOF_THRESHOLD`, `resolve_execution_modes` and `AssemblyMode` (with ε's compatibility wrapper); the dual moves onto the session in the adapter (β or #7453, second to land — C10), and η deletes the single-variant enum when it routes that call through the session-taking `solve_dual_cg`. | No inert axis lingers past the leaf that empties it; β stays the only hot-file leaf of this batch. |

## 5. Contract (the H half)

### C1. The solve seam (`reify_solver_elastic::linear_solve`)

```rust
pub struct SpdSystem { /* private */ }
impl SpdSystem {
    /// `k`: post-BC stiffness, both triangles stored, canonical CSR.
    /// α: `new(k)`; γ adds the required `near_nullspace` argument (every adopter has it).
    pub fn new(k: SparseRowMat<usize, f64>, near_nullspace: NearNullspace) -> Result<Self, SystemInvalid>;
    pub fn n_dofs(&self) -> usize;
    /// Added by ζ (its only consumer): pattern + value bits.
    pub fn content_hash(&self) -> ContentHash;
}
/// γ. Per DOF: the node it belongs to and its displacement component (0..3). The seam builds the
/// six rigid-body columns from `node_coords`, and the AMG block partition from `dof_node`.
pub struct NearNullspace { pub dof_node: Vec<u32>, pub dof_component: Vec<u8>,
                           pub node_coords: Vec<[f64; 3]> }
pub enum SystemInvalid { NotSquare { rows: usize, cols: usize }, NonCanonical,
                         NonPositiveDiagonal { dof: usize } }

pub enum LinearSolverChoice { Auto, Direct, Iterative }
pub struct SolvePolicy { pub auto_direct_budget: u64, pub direct_ceiling: u64,
                         pub auto_direct_work_budget: Option<f64>,   // Q-I; None until γ calibrates it
                         pub amg: AmgParams }
pub struct FactorEstimate { pub n_dofs: usize, pub len_val: u64, pub factor_bytes: u64,
                            pub predicted_flops: f64 }
pub enum Strategy { Direct, Pcg(PreconditionerKind) }      // PreconditionerKind::{Jacobi, SmoothedAggregation}
pub enum StrategyReason {
    AutoWithinBudget { predicted_bytes: u64, budget: u64 },
    AutoOverBudget { predicted_bytes: u64, budget: u64 },
    AutoOverWorkBudget { predicted_flops: f64, budget: f64 },   // Q-I, from γ
    Requested,
}
pub struct StrategyDecision { pub strategy: Strategy, pub reason: StrategyReason }

/// The one home of the tier policy (heuristics 7, 11). Pure.
pub fn choose_strategy(choice: LinearSolverChoice, estimate: Option<&FactorEstimate>,
                       policy: &SolvePolicy) -> Result<StrategyDecision, SolveFailure>;

pub struct SpdSolver { /* the system, the decision, and Factor | Hierarchy | Jacobi state */ }
impl SpdSolver {
    pub fn prepare(system: Arc<SpdSystem>, choice: LinearSolverChoice, policy: &SolvePolicy,
                   ctl: &mut SolveControl<'_>) -> Result<SpdSolver, SolveStop>;
    pub fn decision(&self) -> &StrategyDecision;
    pub fn solve(&mut self, rhs: &[f64], limits: SolveLimits, warm: Option<&[f64]>,
                 ctl: &mut SolveControl<'_>) -> Result<Solution, SolveStop>;
}
/// From ElasticOptions (#8244): `tolerance` is the PCG stop, or Direct's residual bound; `max_iter` caps PCG.
pub struct SolveLimits { pub tolerance: f64, pub max_iter: usize }

pub struct Solution { pub u: Arc<Vec<f64>>, pub report: SolveReport }
pub struct SolveReport {
    pub decision: StrategyDecision, pub n_dofs: usize,
    pub factor: Option<FactorEstimate>,       // Some when the symbolic analysis ran
    pub iterations: Option<u32>,              // None for Direct
    pub true_rel_residual: f64,               // ‖f − K u‖ / ‖f‖ (0 for a zero RHS), every tier
    pub reuse: FactorReuse,
}
pub enum FactorReuse { None,
                      SessionFactor,        // ζ, η: a second RHS on a live session
                      CachedAcrossEvaluations,   // π: a factor carried across evaluations
                      SymbolicAcrossEvaluations } // π: symbolic reused, numeric re-run
pub enum SolveStop {
    Cancelled { phase: SolvePhase },
    NotConverged { iterations: u32, cap: u32, true_rel_residual: f64, tolerance: f64 },  // PCG only
    ResidualAboveTolerance { true_rel_residual: f64, tolerance: f64 },                 // Direct only
    Failed(SolveFailure),
}
pub enum SolveFailure {
    NotPositiveDefinite { site: NpdSite },
    FactorExceedsCeiling { predicted: u64, ceiling: u64 },   // explicit Direct only
    OutOfMemory { requested: u64, phase: SolvePhase },
    WorkerPanicked { phase: SolvePhase },                    // ι
}
pub enum NpdSite { Pivot { dof: usize },            // Direct: faer's non-positive pivot (C2 mapping)
                   Curvature { iteration: u32 },    // PCG: p·Kp ≤ 0
                   CoarseLevel { level: u8, dof: usize } }   // AMG: a non-positive diagonal on a level
```

- **Zero right-hand side** (S8): `solve` with `‖f‖ = 0` returns `u = 0`, `true_rel_residual = 0`,
  `iterations = Some(0)` on PCG or `None` on Direct, and touches neither the factor nor the
  hierarchy. `prepare` is lazy enough for this: a session whose every RHS is zero never factors
  (`prepare` records the decision; the first non-zero `solve` triggers factorisation or setup).
- **What is and is not detected.** An indefinite `K` surfaces as `NotPositiveDefinite` when faer
  meets a non-positive pivot or PCG a non-positive curvature. A merely *singular* PSD `K` — a floating
  part, a mechanism — can factor with tiny positive pivots and return a large rigid-body component with
  a small residual, so the seam cannot promise to see it. Floating parts are therefore caught
  structurally, before the solve, by the adopter (C8's `FeaStiffnessNotPositiveDefinite`, reason
  "unanchored"): `boundary::unanchored_components(n_nodes, connectivity, constrained_nodes)` returns
  one node of every element-connected component holding no constrained node. A mechanism inside an
  anchored component (a part pinned at a single node) is not detected; that limit is documented in the
  stdlib doc, as today's CG has it too.
- `SpdSystem::new` checks shape, canonical CSR and a stored, positive diagonal on every row, and
  returns a typed `SystemInvalid` (today these are CG panics). Value symmetry is O(ulp), not bitwise
  (§2); the Direct tier reads the lower triangle, the PCG loop the whole matrix. A test pins the
  agreement of the two tiers, not bitwise symmetry.
- **Auto** runs the symbolic analysis and calls `choose_strategy` with its estimate (S7). **Direct**
  runs it and fails with `FactorExceedsCeiling` before any numeric allocation if `factor_bytes >
  direct_ceiling`. **Iterative** skips it (`estimate = None`, reason `Requested`).
- `factor_bytes = 8 · len_val + the numeric scratch requirement`; both are known after the symbolic
  step. `predicted_flops = Σ_j (c_j + c_j(c_j + 1)/2)` over the column counts `c_j` of the returned
  structure (faer's own formula, recomputed because faer does not return it; padded columns on the
  supernodal path are counted as stored).
- **Convergence.** The iterative tier stops on the recurrence residual `‖r‖ ≤ tol·‖f‖`, as today
  (P5: the true residual can sit above 1e-6 on a converged thin plate), and returns `NotConverged`
  at the cap. The true residual is computed after every solve on every tier and reported. Direct:
  `converged` iff `true_rel_residual ≤ tolerance` (Q-D), else `ResidualAboveTolerance`.
- `prepare` and `solve` never panic on data; contract violations (a wrong RHS length) still assert.
- The seam is the only caller of faer's Cholesky in reify-solver-elastic besides `SplitCholesky`,
  and both share one driver (C2).

### C2. Direct tier (`linear_solve/direct.rs`)

- `SplitCholesky`'s symbolic + numeric driving is split into one internal driver
  (`analyse(a, params) -> Symbolic`, `factorize(symbolic, a, par) -> Factor`) that both
  `SplitCholesky` and the Direct tier call (heuristic 11). Every faer call in it passes **`Par::Seq`
  explicitly**; no code in reify-solver-elastic reads `faer::get_global_parallelism()` afterwards.
  Eigensolve results are unchanged (the global is `Par::Seq` today).
- Order: symbolic (AMD, `Side::Lower`) → policy check (C4) → `try_reserve_exact` for `len_val` and
  `MemBuffer::try_new` for the scratch, each failure `OutOfMemory { requested }` (out-of-memory comes
  only from these two reservations; `factorize_numeric_llt` itself returns only the dense `LltError`)
  → numeric LLᵀ → composed solve per RHS → true residual.
- **Pivot mapping.** `LltError::NonPositivePivot { index }` is converted to a 0-based permuted
  position by the symbolic variant — `SymbolicCholeskyRaw::Simplicial`: `index − 1`;
  `SymbolicCholeskyRaw::Supernodal`: `index` — and then through the symbolic permutation to a DOF:
  `NotPositiveDefinite { site: Pivot { dof } }`. The permutation's direction is pinned by BT4, which
  runs both variants.
- **Fault-injection hook (α).** Compiled only when reify-solver-elastic's
  `linear-solve-fault-injection` cargo feature is enabled, and inert unless a test arms it. Armed, it
  can count reservations, symbolic calls and numeric calls (the symbolic counter is consumed by π's BT33), fail a reservation, hold a factorisation worker at a
  barrier before faer's numeric call (ι), or panic in it (ι). Test targets enable the feature through
  dev-dependencies. Whether cargo's feature unification under `cargo test` also compiles it into other
  test binaries of the workspace (the CLI's, the GUI's) is **unverified**; it would be inert there
  either way, and release builds do not enable it.
- Under E′ (C5) the symbolic and numeric calls run on the detach worker; the composed solve and the
  residual check run on the calling thread and poll cancel per RHS.

### C3. AMG tier (`linear_solve/amg/`, γ)

- **Setup** from the prototype's measured configuration: the near-nullspace is the six rigid-body
  columns (three translations, three rotations about the centroid of `node_coords`) evaluated at each
  DOF's `(node, component)` (S15); the **block partition** groups DOFs by `dof_node` — a node's block
  holds only the DOFs the system kept, so a node with an eliminated component has a smaller block; the
  strength graph is on that partition, with θ = 0.05 on the Frobenius norms of the off-diagonal blocks
  against the diagonal blocks, ignoring stored zeros; greedy three-pass aggregation in node order;
  tentative prolongator from a per-aggregate rank-revealing QR of the near-nullspace; prolongator
  smoothing with ω = 4/(3ρ), ρ = 1.05 × a 30-step power estimate of ρ(D⁻¹A) from a fixed start
  vector; Galerkin coarse operator; rows with no non-zero off-diagonal (clamped DOFs) are not smoothed.
- **Coarsest level**: ≤ `coarse_max` (4,000) DOF or 12 levels, or a level that would shrink by
  < 10 % (the stall guard, P3). It is factored by the **Direct tier** (C2), inline (small).
- **Cycle**: V(1,1) with forward Gauss–Seidel before and backward after — a symmetric preconditioner,
  so PCG is valid.
- Everything is sequential and in a fixed order: deterministic by construction (Q-B).
- **Warm start** (D-3): `solve` accepts an initial guess on this tier only; the adapter still gates
  it with `!deterministic && warm_start_beneficial` (#4869).
- **Cancel and progress**: `Phase(AmgSetup { level })` on each level, polled per level and per PCG
  iteration. A non-positive diagonal on any level is `NotPositiveDefinite { site: CoarseLevel }`.
- `AmgParams { theta, coarse_max, max_levels, stall_ratio }` lives in `SolvePolicy`, engine-configured
  with these defaults and no `.ri` surface.

### C4. Policy and budgets

- `SolvePolicy::default()`: `auto_direct_budget = 2 GiB`, `direct_ceiling = 8 GiB`,
  `auto_direct_work_budget = None` until γ sets the calibrated value (Q-I), the C3 defaults. `Engine::with_solve_policy(SolvePolicy)`
  (new, β) overrides it from Rust (engine tests: BT13c, BT14, BT26); there is no `.ri` knob (Q-C) —
  the pattern meshing-service C10 specifies for its `SizePolicy`.
- `choose_strategy`: Auto → `Direct` iff `factor_bytes ≤ auto_direct_budget` and (Q-I, once set)
  `predicted_flops ≤ auto_direct_work_budget`, else `Pcg(SmoothedAggregation)` (before γ:
  `Pcg(Jacobi)`, S3). Direct → `Direct` iff `factor_bytes ≤ direct_ceiling`, else
  `FactorExceedsCeiling`. Iterative → `Pcg(SmoothedAggregation)` (before γ: `Jacobi`).
- **Q-I: the predicted-work term (ruled).** Q-C's budget is a memory bound, and P3 shows AMG
  faster on chunky bodies well under it. Auto therefore also requires `predicted_flops ≤
  auto_direct_work_budget`, computed deterministically from the symbolic structure (C1). γ calibrates
  the work budget from the measured families so that Direct is chosen only where its numeric time is
  at most AMG's setup + solve, and records the calibration (cases, flops, both times). Before γ the
  term stays `None`: the over-budget tier is still Jacobi, so the term would only send work to a
  slower tier. Engine-level, no `.ri` knob.
- The decision reads nothing else — no free memory, no load, no ledger state (Q-B, S12).
- **How a trampoline gets the policy.** `ComputeFn` has no engine parameter, so `run_compute_dispatch`
  installs the engine's `SolvePolicy` into the solve dispatch context (`install_solve_dispatch_context`)
  beside the progress sink and the `CancellationHandle` (β); the elastic adapter and the buckling
  trampoline (θ) read it there. A caller with no dispatch context (direct Rust callers, mesh-morph at the
  realization edge) uses `SolvePolicy::default()`.

### C5. Cancel and progress (Q-E, E′)

```rust
pub enum SolvePhase { Assemble, Analyse, Factor, AmgSetup { level: u8 }, Iterate, Substitute,
                      Verify, AwaitOrphan }
pub enum SolveEvent { Phase(SolvePhase), Iteration { iter: u32, residual: f64 }, Heartbeat }
pub struct SolveControl<'a> {
    /// Progress + cooperative cancel poll; `ControlFlow::Break(())` cancels.
    pub on_event: &'a mut dyn FnMut(&SolveEvent) -> core::ops::ControlFlow<()>,
    pub detach: Option<Arc<FactorLedger>>,                   // added by ι; Some iff a cancel source exists
}
pub struct FactorLedger { /* one per Engine; ι */ }
```

- **Cooperative polls** (each returns `Cancelled { phase }` on `Break`): every phase entry, assembly
  per element chunk (C12, through its own poll), AMG setup per level, every PCG iteration, the
  substitution per RHS, the residual check. `Phase` fires once per phase entry; `Iteration` keeps
  today's cadence at the engine edge (iteration 1, then every `PROGRESS_STRIDE`); `Heartbeat` fires
  only from the detach wait loop (ι), at a fixed short interval.
- **The engine edge from β.** The adapter maps each `Phase` entry to a `SolverProgressUpdate` in
  **today's** wire shape — `solver_kind: "direct" | "jacobi-pcg" | "amg-pcg"`, `iter: 0`,
  `residual: 1.0` (the true initial relative residual of a cold solve, `‖f − K·0‖/‖f‖ = 1`, so not a
  fake value) — and each `Iteration` as today. The overlay's debounce therefore starts at the first
  phase and Cancel is reachable on a Direct solve from β on. Until ι, a cancel during an inline
  factorisation takes effect at the next phase boundary (§7 names this window).
- **Detach (ι).** With `detach = Some(ledger)`, the symbolic and the numeric factorisation each run on
  a dedicated worker thread that owns an `Arc` of the CSC matrix and an `Arc<FactorLedger>`, and
  returns its result over a channel. The calling thread waits in a loop that calls
  `on_event(Heartbeat)` at a fixed short interval; on `Break` it returns `Cancelled` at once and the
  worker becomes the ledger's **orphan**, whose result is dropped when it completes. With
  `detach = None` (CLI, tests, callers with no cancel source) the calls run inline.
- **The ledger (ι)** is owned by the `Engine` and installed into the dispatch context next to the
  progress sink and the `CancellationHandle` (per evaluation, per the #5215/#7438 ruling — never a
  process global). It holds at most one orphan: its predicted bytes and start time.
  - **Admission** (S12): with no orphan, a factorisation runs detached. With an orphan, it proceeds
    **undetached** iff the orphan's bytes plus its own ≤ `direct_ceiling`; otherwise it emits
    `Phase(AwaitOrphan)` and waits (polling `Heartbeat`) until the orphan finishes, then runs detached.
    The symbolic step, whose memory is O(nnz(K)), runs undetached whenever an orphan exists. The
    decision of *which tier* is never revisited (Q-B).
  - **Release on every exit.** The orphan slot is released by a drop guard on the worker thread, so a
    completion, an `Err`, an out-of-memory return or a panic all free it. A waiter that is cancelled
    while in `AwaitOrphan` leaves the orphan registered. Because each worker holds an
    `Arc<FactorLedger>`, the ledger outlives the `Engine` if an orphan is still running at drop.
  - The worker body runs under `catch_unwind` (`WorkerPanicked`) with fallible allocation (C2). When
    an orphan completes, the ledger emits a `tracing` debug event naming its bytes and run time, so
    "nothing visible running" is never "CPU silently busy". A later solve that must wait shows
    `AwaitOrphan` in the overlay.
- **The GUI wire (κ).** `SolverProgressUpdate` becomes `{ solver_kind, phase: &'static str, iter:
  Option<u32>, residual: Option<f64> }`; the Tauri `SolverProgress` struct gains `phase` and makes
  `iter`/`residual` optional. The overlay renders any phase name it does not know generically (its
  raw name and elapsed time), so a phase added later needs no GUI change.

### C6. `ElasticOptions.linear_solver`, `max_iter` and `cg_tolerance` (Q-D)

```
enum LinearSolver { Auto, Direct, Iterative }
structure def ElasticOptions { …  param adaptive : Bool = false
                                   param linear_solver : LinearSolver = LinearSolver.Auto }
```

- `linear_solver` is **appended after `adaptive`**, the last declared param (constructors bind by
  name since #4522, but appending keeps every existing positional reading valid). β moves the param
  count pin in `solver_elastic_tests.rs` from 17 to 18 and corrects the leading "sixteen params"
  comment.
- `extract_linear_solver(options)` lives in the adapter; every lane reads it, and `solve_load_cases`
  honours a per-case override as it does for every other option.
- **Per tier**: Iterative — `max_iter` caps PCG iterations and `cg_tolerance` is the recurrence
  stopping tolerance, both with #8244's landed defaults and its coded cap-exhaustion Error. Direct —
  `cg_tolerance` bounds the post-solve true residual; `max_iter` does not apply and, when off its
  landed default, raises #7079's `ParamNotApplicable` naming `ElasticOptions.max_iter`, the Direct path
  and the reason (S14, δ).
- **Param-drop declaration** (#7080's C1): `linear_solver` is honoured, so the union becomes 18.
  `max_iter`, `cg_tolerance`, `deterministic` and `threads` stay honoured with the meanings below. If
  #7080 has landed when β lands, β adds `linear_solver`; otherwise #7080 includes it (amendment, §7).
- **Stdlib doc text** (β rewrites it):
  - `deterministic` — "always the same answer (bit-identical), regardless of the path taken to get
    there. A direct solve already is; on the iterative tier `true` disables warm start from earlier
    evaluations, which `false` (the default) keeps for speed. Reuse that gives a bit-identical answer
    (a shared factor) is still allowed. Results are bit-identical across runs and `threads` values on
    one machine, and tolerance-equivalent across machines" (Q-H).
  - `threads` — "worker threads for stiffness assembly; never changes a result bit".
  - `max_iter`/`cg_tolerance` — the per-tier meanings above.
  - `iterations` — "PCG iterations; 0 for a direct solve".
  - `converged` — "the solve met `cg_tolerance` (the true residual on Direct)".
  - `linear_solver` — the three variants, the engine policy, and the mechanism limit of C1.

### C7. Determinism (Q-B)

- **Contract** (the stdlib doc and `solver.rs`'s module doc say exactly this): every tier is
  single-threaded and bit-identical across runs, processes and `threads` values on one machine;
  across machines, tolerance-equivalent (P7). No test compares bits against a committed constant.
- **Deleted.** β: every `elastic_static.rs` use of `resolve_execution_modes`, `SolverMode` and
  `PARALLEL_DOF_THRESHOLD` (calls, its tests such as
  `synthetic_grid_counts_bounds_thin_body_dofs_below_parallel_threshold`, and `NX_MAX`'s
  `PARALLEL_DOF_THRESHOLD` rationale — the clamp now bounds DOF only). ξ: `SolverMode::Parallel`, the
  parallel CG helpers (`spmv_parallel`, `dot_parallel` and siblings), `PARALLEL_DOF_THRESHOLD`,
  `resolve_execution_modes`, the `assemble_global_stiffness` wrapper and `AssemblyMode` (C12), with
  their reify-solver-elastic tests.
- **Tests that change:**
  - `tests/determinism.rs` (reify-solver-elastic; today it drives `resolve_execution_modes` and
    `solve_cg` directly) is rebuilt by ξ on the seam and ε's assembly: Direct and the iterative tier,
    bitwise equal across `threads ∈ {1, 4, 16}`.
  - `solver_gate_smoke.rs` and `solve_elastic_static_e2e.rs` drop their `Parallel` references (ξ).
  - `fdm_progressive_refinement_e2e.rs::deterministic_pins_one_rung_and_is_bit_stable` holds
    unchanged.
  - `constant_field_lift_matches_isotropic_elastic_result`'s iteration equality holds (both 0 under
    Direct; β).
  - `heterogeneous_warmstart_integration.rs` calls `solve_cg_warm` directly, so β does not touch it;
    it stays green under S11 (γ).
  - The reify-gui engine test that asserts `solver_kind == "cg"` and `iter ≥ 1` on a progress sink
    (`gui/src-tauri/src/tests/engine_tests.rs`) is updated by β to the phase-entry shape.

### C8. Diagnostics (every Warning and Error carries a `DiagnosticCode`, INV-SF-6)

| Code | Severity | Emitted by | When |
|---|---|---|---|
| `FeaLinearSolveStrategy` | Info | the adapter; buckling | every static solve: strategy, reason, DOFs, predicted factor bytes and the budget (or "requested"), iterations, true residual, the `FactorReuse` ("factor reused (case k)" ζ, η; "factor reused (cached)" / "symbolic analysis reused (cached)" π) — deterministic text (S13) |
| `FeaStiffnessNotPositiveDefinite` | Error | the adapter; buckling | an unanchored component (C1; names one of its nodes and the component's node count), `NotPositiveDefinite` or `SystemInvalid::NonPositiveDiagonal` — names node, axis and coordinates where a DOF is known, the iteration otherwise; replaces today's CG panic |
| `FeaDirectFactorTooLarge` | Error | the adapter | explicit Direct whose predicted factor exceeds the ceiling — both byte counts |
| `FeaDirectResidualAboveTolerance` | Error | the adapter; buckling | `ResidualAboveTolerance` — the residual and the tolerance |
| `FeaSolverOutOfMemory` | Error | the adapter; buckling | `OutOfMemory` — phase and requested bytes |
| `FeaSolverWorkerPanicked` | Error | the adapter | `WorkerPanicked` (ι) |
| #8244's cap-exhaustion code | Error | the adapter | `NotConverged` (PCG), naming the strategy |
| `ParamNotApplicable` (#7079) | Warning | the adapter (δ) | `max_iter` off its default on a solve that resolved to Direct (S14) |

`FeaStiffnessNotPositiveDefinite` and the existing `FeaSingularStiffness` do not overlap.
- `FeaSingularStiffness` stays the **geometric, pre-solve** finding: elements of near-zero volume
  (`classify_degenerate`; its threshold is #8254's).
- `FeaStiffnessNotPositiveDefinite` is the **algebraic** finding on a mesh that passed that check:
  a component with no constraint, or a non-positive pivot, curvature or diagonal met by the solve.
- A degenerate element is reported by the first and never reaches the solve.

Each code is minted by the leaf whose own test first needs it; every Error makes the solve
`ComputeOutcome::Failed`, so constraints reading it go INDETERMINATE and `reify eval` exits non-zero.
Mesh-morph maps `SolveStop` onto its existing `ElasticityFailure` (C11).

### C9. Cache identity

- **Persistent FEA cache**: every Jacobi-era entry misses once β lands, by `ENGINE_VERSION_HASH`
  (S10). An entry's `ElasticResult` keeps `iterations`/`converged` with C6's meanings;
  `ELASTIC_RESULT_FORMAT_VERSION` does not change (no field changes). The strategy Info replays with
  the entry (#7245's `WithDiagnostics`). Because `Engine::persistent_cache_key` hashes evaluated
  argument values, `linear_solver` reaches the on-disk key from β on at a direct dispatch; the body
  redispatch path's constant key is #7052's.
- **In-process NodeCache — known limitations, owned by #7052.** Its structural key carries
  `options_hash = ContentHash(0)`, which gives two holes.
  - Toggling `linear_solver` in a GUI session can be served the previous tier's result, with the
    previous `iterations` and Info.
  - A `deterministic: true` request can be served an earlier warm-started iterative result, which
    breaks Q-H in-process. The persistent key already hashes argument values, so it is not affected.
  - Both close when #7052 lowers options into the key. #7052 must therefore **include**
    `linear_solver` and `deterministic` in that hash. `threads` may stay excluded: it is now
    bit-invisible by construction (S6). Amendment in §7.
- `SolvePolicy` is engine configuration, not an option: an engine with a non-default policy is a test
  engine, and a result from another policy is tolerance-equivalent (Q-B).

### C10. Factor reuse (ζ, η)

- `solve_multi_case_trampoline` keeps one `SolverReuse` scope per call: a map from
  `(SpdSystem::content_hash, LinearSolverChoice)` to a prepared `SpdSolver` (S9). Each case still
  meshes (the body path already shares one realization, #4152) and assembles; a case whose key is
  present solves its RHS on the existing session, so N cases with one support set cost one
  factorisation and N substitutions. The Info says "factor reused (case k)". The scope dies with the
  call. ζ records the per-case assembly cost.
- **Where support grouping is exercised.** On the dims (synthetic) path the supports list does not
  reach `K`: `solve_cantilever_fea` always clamps the root face (`SYNTHETIC_CLAMP_FACE`), and the list
  only drives the "insufficient supports" advisory (D3, §7). Cases there differ in `K` only through
  geometry, material or options, so BT21 differentiates by `linear_solver`. Grouping by support set is
  exercised where supports reach the system — the body overload with selector-resolved node sets (μ's
  territory, #5313) — and the content-hash key (S9) makes it correct there by construction: equal
  supports give an equal `K` and share a factor; different ones do not.
- **DWR dual** (after #7453): the primal's `SpdSolver` stays alive past the primal solve; the dual is
  a second `solve` on it with `g` zeroed at the constrained DOFs. #7453 writes its dual as a direct
  `solve_cg(…, SolverMode)` in `elastic_static.rs` (not through `solve_dual_cg`), and its order against
  β is free. So **whichever of β and #7453 lands second puts the dual on the adapter's session**: β
  moves #7453's call into the adapter with the primal it is already moving, or #7453, amended, writes
  its dual there through the session. Either way the dual ends up in the adapter, outside the hot file.
  η then routes that call through `solve_dual_cg`, which takes the session instead of `(k, opts, mode)`
  and stays the one home of the homogeneous dual data. η also deletes the now single-variant
  `SolverMode`.
  Goal-oriented-error-estimation §5.3's "a second CG solve of the same cost as the primal" becomes "a
  second right-hand side on the primal's factor or hierarchy".
- The adaptive lanes get no cross-iteration reuse (a remesh changes the DOF numbering).

### C11. Other adopters (θ)

- **Buckling pre-stress** (`solve_buckling_kernel`, `solve_buckling_kernel_p2`): `K_red`
  (`project_with_expansion`) becomes an `SpdSystem` whose `NearNullspace` gives each reduced DOF the
  `(node, component)` of the full DOF it enumerates (S15); Auto; the `assert!` on non-convergence
  becomes a typed `BucklingKernelError::PreStress(SolveStop)` that the buckling trampoline maps to
  C8's codes. The eigensolve's own factorisation is unchanged.
- **Mesh-morph elasticity** (`reify-mesh-morph/src/elasticity.rs`): the post-BC `K` (inhomogeneous
  Dirichlet lifted into `f`) becomes an `SpdSystem` over the old mesh's vertices; Auto; `SolveStop`
  maps onto `ElasticityFailure` (`SolverNotConverged` for `NotConverged`, new variants for the rest).
  `elasticity_morph_with_cg_opts` takes a `LinearSolverChoice` and `SolveLimits` in place of
  `CgSolverOptions` (θ updates the cite in reify-audit's `engine_seam_g_allow_cites_live.rs`
  allowlist); its tests that drive non-convergence with `max_iter = 1` select `Iterative`. A tick with
  zero prescribed motion is a zero RHS (S8). Every surface node is pinned, so the unanchored check
  never fires. Morph's user-observable path is meshing-service ε (#8288, its BT11); θ's own signal is
  the crate's morph tests. Factor caching across morph ticks stays #7836's decision.
- **Shell, membrane, tensegrity** stay on `solve_cg` (D-4), which γ re-expresses on the shared PCG
  loop with `Jacobi` (S11).

### C12. Assembly (Q-F, ε)

- `assemble_stiffness(n_nodes, elements, threads: NonZeroUsize, poll: &mut dyn FnMut() ->
  ControlFlow<()>) -> ControlFlow<(), SparseRowMat<usize, f64>>` is the new primitive. It shares only
  std's `ControlFlow` with α's seam (S4), so ε and α are independent. Steps:
  the CSR pattern from connectivity once (per node, the sorted union of its elements' nodes, expanded
  to the element DOF stride); a node → incident-elements index in element order; values preallocated
  at the pattern's size and filled row by row, each entry summed over incident elements in element
  order from `+0.0` (S6). Rows are partitioned across `threads` by node; no triplet buffer, no
  `BTreeMap`. `poll` is called per element chunk.
- **Invariants**: the result is bitwise equal at every thread count and bitwise equal to today's
  Deterministic arm on the same element slice (same entries, same summation order). Mixed-DOF
  (shell/tet) and orphan-DOF behaviour, `detect_orphan_dofs`, and every contract panic are unchanged.
- Peak memory ≈ the final CSR plus the caller's element-stiffness store.
- **Migration without touching the hot file.** ε re-implements `assemble_global_stiffness(n_nodes,
  elements, AssemblyMode)` as a thin wrapper over `assemble_stiffness` (`Deterministic` → one thread,
  `Parallel { threads }` → `threads`, a never-breaking poll), so every caller keeps compiling and every
  caller's `K` becomes the node-owned result at once — including `solve_cantilever_fea`'s Parallel arm,
  now bit-identical to its Deterministic arm. β calls `assemble_stiffness` from the adapter (with
  `threads` from `ElasticOptions` and the cancel poll). ξ moves the remaining callers — `modal_ops`,
  `buckling_kernel`, reify-mesh-morph's elasticity, `shell_solve`, `membrane_load`, `tensegrity_load`,
  `assembly/volume.rs` — to `assemble_stiffness` with one thread, and deletes the wrapper and
  `AssemblyMode`.

### C13. Warm state

- AMG tier: unchanged semantics (`CgWarmState` donated after every solve, read iff `!deterministic &&
  warm_start_beneficial`). Direct tier: a donated warm state is ignored, `warm_started = false`, and
  the fresh `u` is still donated (a later over-budget solve can use it). `CgWarmState` keeps its name
  and registration.
- `warm_state.rs`'s header ("direct-solve symbolic-factorization caching is out of scope for v0.3") is
  replaced by a pointer to C14.
- **Ownership of the warm-state value (β).** The trampoline in `elastic_static.rs` passes the prior
  `OpaqueState` to the adapter untouched and donates whatever the adapter returns. The adapter owns the
  concrete warm-state type and its downcast. This keeps π, which changes that type, out of the hot file.

### C14. Numeric-factor cache across evaluations (Q-J, π)

- **What is cached.** After a Direct solve, the adapter's warm-state value carries the prepared
  session's numeric factor, together with its symbolic analysis and two keys, beside the `CgWarmState`
  iterate:
  - `(SpdSystem::content_hash, LinearSolverChoice)` — ζ's hash of the post-BC pattern and value bits;
  - `SpdSystem::pattern_hash` — the CSR row pointers and column indices only, added by π.
  The value travels the existing warm-state path: the compute node's prior `OpaqueState`, then the
  engine's `WarmStatePool` (compute-node-contract §4).
- **API (π).** `SpdSolver::into_cached(self) -> Option<CachedFactor>` (Direct sessions only) and
  `SpdSolver::prepare_from_cache(system, choice, policy, cached, ctl)`, which applies the rules below
  and reports the `FactorReuse` it achieved.
- **Reuse rules.** On the next evaluation of the same node, the adapter builds the new `SpdSystem`
  and compares keys.
  - An equal content hash and choice means the cached factor is reused: no symbolic, no numeric, one
    substitution. This is sound by construction (heuristic 10) — an equal hash means an equal
    post-BC `K`, so the answer is bitwise the fresh solve's (same factor, same RHS).
  - An equal pattern hash with different values (a material or thickness edit that keeps the mesh)
    reuses the symbolic analysis and runs a new numeric factorisation. The AMD ordering and the
    structure depend only on the pattern, so this too is bitwise a fresh solve.
  - Anything else factors from scratch.
- **The Info.** It says "factor reused (cached)" or "symbolic analysis reused (cached)".
- **Determinism (Q-H).** Both reuses give bit-identical answers, so they are permitted under
  `deterministic: true`. The adapter therefore reads the prior warm state for its factor even when
  `deterministic` forbids the iterative warm start.
- **Memory.** The value's `estimated_size_bytes` is `8n + factor_bytes` (C1). It counts against the
  pool's existing budget (`WarmStatePool`, default 2 GiB, `REIFY_WARM_STATE_BUDGET_BYTES` or config),
  so the pool's cost-weighted eviction bounds memory. The pool keeps a single item larger than its
  whole budget (its documented "over by one item" rule), so a factor near the 2 GiB Auto budget stays
  cached until another donation evicts it. An evicted entry costs one factorisation on the next solve.
- **Eviction cost.** The pool evicts the lowest `cost_per_byte` first, and today's elastic trampoline
  sets it to `1 / size_bytes`, which would make a large factor the first casualty. π sets a factor-
  carrying value's cost from the work it saves — `predicted_flops / estimated_size_bytes` (α's
  estimate) — so an expensive factor outlives a cheap one of equal size. BT34 holds either way.
- **Not cached:**
  - **AMG hierarchies.** Setup depends on the matrix values (strength graph, smoothed prolongator,
    Galerkin products), so it cannot be reused across a value change. On an unchanged `K` the cheap
    case is already served on the iterative tier by the warm start.
  - **A factor produced by an orphan** (C5) is dropped and never cached. Only a factor returned to a
    live solve is donated.
  - **Factors from `solve_load_cases`** (it donates no warm state; ζ's reuse is within one call) and
    **from the adaptive lanes** (every remesh changes the pattern).
  - **The persistent cache** never holds a factor; this cache is in-memory only.
- **Not here:** morph's factor caching across ticks stays #7836's decision.

## 6. Boundary-test sketch (both sides of the seam)

CLI rows run `reify eval` from a reify-cli integration test, **each invocation with a fresh
`REIFY_CACHE_DIR`** (the shared harness sets none); "engine" rows are reify-eval integration tests
through `Engine`; "seam" rows are reify-solver-elastic integration tests against the public
`linear_solve` API. The **thin plate** is the dims overload, 800 × 500 × 12 mm, `ShellForce.Off`, tip
load 1000 N, root clamp (5,082 DOF). The **smoke cantilever** is `examples/fea_cantilever_smoke.ri`'s
1000 × 100 × 100 mm beam (2,562 DOF). No row asserts a wall-clock time or a committed bit pattern;
bit equality is compared within one test run.

| # | Scenario | Precondition | Postcondition | Leaf |
|---|---|---|---|---|
| BT1 | Direct solves an SPD system | Seam: the smoke cantilever's post-BC `K`, `Direct` | `Solution` with `iterations = None`, `true_rel_residual ≤ 1e-10`, `factor` reported | α |
| BT2 | Auto reads the estimate | Seam: one system, policies with budgets just above and just below its `factor_bytes` | `AutoWithinBudget` → `Direct`; `AutoOverBudget` → `Pcg(Jacobi)`; both decisions name the same predicted bytes | α |
| BT3 | Explicit Direct over the ceiling | Seam: ceiling below the system's `factor_bytes` | `Failed(FactorExceedsCeiling)` with both counts; no numeric reservation (α's fault hook counts them) | α |
| BT4 | An indefinite pivot names its DOF | Seam: an identity system with one indefinite 2 × 2 block `[[1, 2], [2, 1]]` at known DOFs, factored once with `FORCE_SIMPLICIAL` and once with `FORCE_SUPERNODAL` (`supernodal_flop_ratio_threshold`) | Both: `Failed(NotPositiveDefinite { site: Pivot { dof } })` with `dof` in that block (exact: either elimination order gives pivots 1 then −3); the two arms name the same DOF | α |
| BT4b | A zero right-hand side | Seam: `prepare` with Direct and with Iterative, `solve(0)` | `u = 0`, `true_rel_residual = 0`, converged; the fault hook counts no numeric call | α |
| BT5 | Two solves, one session | Seam: `prepare` once, two RHS | Both solve; one factorisation (fault hook counts numeric calls) | α |
| BT6 | Direct never reads the global | grep over `crates/reify-solver-elastic/src` | No `get_global_parallelism` call construct remains; `tests/split_cholesky.rs` and `tests/eigensolve_shift_contract.rs` green unchanged | α |
| BT7 | Assembly is thread-invariant | Seam: a gmsh-free unstructured P1 mesh and a P2 mesh; `threads ∈ {1, 2, 8}` | `K` bitwise equal across all three | ε |
| BT7b | Assembly is cancellable | Seam: a poll that breaks on its third call | `ControlFlow::Break(())` returned; no matrix | ε |
| BT8 | Assembly matches today | ε's diff carries one comparison run against the pre-ε Deterministic arm on three fixtures, plus a recorded assembly time on the R-meshes' size class | Bitwise equal (recorded in the PR; the old arm is then deleted) | ε |
| BT9 | The thin plate converges (CLI) | Thin plate with cells for `result.converged` and `result.iterations` | `converged = true`, `iterations = 0`; `FeaLinearSolveStrategy` names Direct, 5,082 DOFs, predicted bytes and the 2 GiB budget; exit 0. (Before β: `converged = false`, `iterations = 2000`, §3 P1; after #8244 alone: its coded Error.) | β |
| BT10 | `threads` never changes a bit | Engine with in-process gmsh (as BT11): BT30's plate with a hole, meshed ONCE in-process into a `SolverMesh` (≈ 136k DOF at the seed on #8244's measurement, well above the 10,000 at which today's `resolve_execution_modes` went parallel); that one mesh is then solved through `solve_cantilever_fea` (BT13's provided-mesh shape) at `threads ∈ {1, 4, 16}`, `deterministic: false`. Meshing once removes gmsh's own run-to-run variation from the comparison (the plain realization producer still meshes at host parallelism until meshing-service β #8285 pins it). The test first asserts the strategy Info's DOF count > 10,000, so it cannot pass on a system the old threshold would have kept serial. (The dims fixtures are ≤ 5,082 DOF and never reached the parallel arm.) Heavy partition | Displacement and stress fields bitwise equal across the three | β |
| BT11 | Every lane reports its strategy | Engine: dims, heterogeneous, body (engine-installed gmsh as reify-eval tests do), uniform and realized adaptive lanes | One `FeaLinearSolveStrategy` per solved mesh on each | β |
| BT12 | Cancel is a variant | Engine, three runs: cancel handle set before the solve; `linear_solver: Iterative` cancelled at its first iteration event; Auto → Direct cancelled at its `Analyse` phase event | `ComputeOutcome::Cancelled` all three times, no stress recovery; the construct `!converged && iterations < max_iter` is gone from `elastic_static.rs` (`solve_cantilever_fea_cancelled_skips_stress_recovery` is the behavioural cover) | β |
| BT12b | Direct solves show the overlay's trigger | Engine: a recording `SolverProgressSink` on an Auto → Direct solve | One update per phase entry with `solver_kind = "direct"`, `iter = 0`, `residual = 1.0`; the GUI engine test's updated assertion green | β |
| BT13 | A floating part is coded, not a panic | Engine: `solve_cantilever_fea` on a provided `SolverMesh` with a disconnected free tet | `FeaStiffnessNotPositiveDefinite` (unanchored) naming a node of that tet; `Failed`; no solve attempted | β |
| BT13b | #8244's cap still bites | #8244's `fea_cg_cap_exhausted.ri` with `linear_solver: LinearSolver.Iterative` added | Its coded Error, exit non-zero; `fea_cg_cap_sufficient.ri` unchanged and green | β |
| BT13c | Explicit Direct ceiling (engine) | Engine with `with_solve_policy` ceiling below the smoke cantilever's factor; `linear_solver: Direct` | `FeaDirectFactorTooLarge` with both byte counts; `Failed` | β |
| BT13d | Out of memory is coded (engine) | α's fault hook fails the factor reservation | `FeaSolverOutOfMemory` naming the phase and bytes; process alive | β |
| BT13e | A load-free case is not an Error (engine) | The smoke cantilever with every load zero | `converged = true`, zero displacement, no Error | β |
| BT14 | Over budget goes to AMG (engine) | Engine with `auto_direct_budget` below the smoke cantilever's factor and `coarse_max` lowered (200) so the hierarchy has ≥ 3 levels (at the default 4,000 the 2,562-DOF beam is a one-level AMG, i.e. a direct solve) | Info names `amg-pcg`, reason `AutoOverBudget`; `converged = true` within the default `max_iter`; one `AmgSetup { level }` phase per level; the level count and iterations are recorded | γ |
| BT15 | AMG agrees with Direct | Engine: BT14's engine, Direct vs `Iterative` | Relative 2-norm displacement difference ≤ 1e-5 (basis P5: 5e-9–3.4e-7 at tol 1e-6 on every measured family; the leaf records the value) | γ |
| BT16 | The stall guard holds | Seam: a structured box whose clamped nodes stall coarsening (B200k-shaped at a small size) | Setup completes; the stalled level is the coarsest; solve converges | γ |
| BT17 | AMG is deterministic | Seam: two `prepare`+`solve` runs in one test | Bitwise equal iterates and iteration counts | γ |
| BT17b | Jacobi path on the shared loop | `solve_cg` callers' existing tests (shell, membrane, tensegrity, `solver.rs`, `heterogeneous_warmstart_integration.rs`) | Green unchanged; `solve_cg`'s panics and zero-RHS return preserved | γ |
| BT17c | Q-I calibration recorded | γ's measurement on R1–R5, T1, B20–B41 | The chosen `auto_direct_work_budget`, each case's flops and both tiers' times recorded in the PR and the policy doc; no test asserts a time | γ |
| BT18 | Iterative is selectable (CLI) | Smoke cantilever, `linear_solver: LinearSolver.Iterative` | `iterations ≥ 1`, `converged = true`; Info names the iterative strategy (`jacobi-pcg` at β, `amg-pcg` from γ), reason `requested` | β, then γ |
| BT19 | `max_iter` under Direct warns (CLI) | Thin plate with `max_iter: 50` (Auto → Direct) | `ParamNotApplicable` naming `ElasticOptions.max_iter` and the Direct reason; `converged = true`; exit 0. The same fixture with `linear_solver: Iterative` gets no such warning | δ |
| BT20 | `max_iter` at default is silent | Thin plate, no `max_iter` | No `ParamNotApplicable` (C3 of the param-drop PRD) | δ |
| BT21 | One factor per distinct system (CLI) | `solve_load_cases` on the smoke cantilever (dims path): cases A and B with different loads and default options; case C with per-case options `linear_solver: LinearSolver.Iterative` (on the dims path supports do not change `K` — `SYNTHETIC_CLAMP_FACE` always clamps the root — so a support difference is not a differentiator here) | Three strategy Infos: A's says Direct, B's says "factor reused (case A)", C's names the iterative strategy (`jacobi-pcg` before γ, `amg-pcg` after) with reason `requested` — its key `(content hash, Iterative)` differs from A's; B's displacement equals a single-case solve of B bitwise | ζ |
| BT22 | DWR dual reuses the primal (engine) | #7453's goal-oriented fixture | Each iteration's dual Info says "factor reused"; the dual solution is bitwise equal to a fresh session's dual solve on the same `K` in the same test (same factor, same RHS) | η |
| BT23 | Buckling through the seam | CLI arm: `examples/buckling_column_p2.ri` under the default policy. Engine arm: the same model on an engine whose `auto_direct_budget` is lowered via `Engine::with_solve_policy` (β) below its `K_red` factor (`BucklingOptions` gains no `linear_solver`) | CLI: a `FeaLinearSolveStrategy` line from the buckling trampoline naming Direct; critical load within the example's existing tolerance. Engine: the Info names `amg-pcg`, reason `AutoOverBudget`, on the reduced system; critical load within the same tolerance of the CLI run | θ |
| BT24 | Morph through the seam | reify-mesh-morph's elasticity tests through `elasticity_morph` | Morphed meshes produced as before (their existing quality assertions); `elasticity_morph_is_deterministic_across_runs_with_same_input` green; `Iterative` with `max_iter = 1` still yields `ElasticityFailure::SolverNotConverged`; reify-audit's `engine_seam_g_allow_cites_live.rs` green | θ |
| BT25 | Cancel during a factorisation returns at once (engine) | α's fault hook holds the numeric worker at a barrier; cancel fires | `ComputeOutcome::Cancelled` returned while the barrier is still closed | ι |
| BT26 | Admission by memory (engine) | BT25's orphan still held; a second solve. Arm (a): ceiling large enough for both. Arm (b): ceiling below the sum | (a) No `AwaitOrphan`; the second solve completes while the orphan is still held, its factorisation running on the calling thread (the hook records the thread), bits equal to an orphan-free run. (b) It emits `AwaitOrphan` and does not factor; on barrier release it proceeds detached and returns bits equal to an orphan-free run. Both choose the same strategy as an orphan-free run | ι |
| BT27 | A worker panic is coded (engine) | Arm (a): the hook panics in a live worker. Arm (b): it panics in an orphan (after BT25's cancel) | (a) `FeaSolverWorkerPanicked`; the engine serves the next solve. (b) The next solve on that Engine is admitted without `AwaitOrphan` (the drop guard released the slot) | ι |
| BT28 | Phases are reported (engine) | A recording `SolverProgressSink` on a Direct and an Iterative solve | Direct: `Assemble, Analyse, Factor, Substitute, Verify` by name; Iterative: `Assemble`, then `Iterate` with iteration events, then `Verify` (`AmgSetup` levels between them once γ has landed — BT14 covers those) | κ |
| BT29 | The overlay shows phases (GUI) | vitest on `SolverProgressOverlay` with the new wire payloads (including an unknown phase name); the reify-gui engine test recording `(solver_kind, phase, iter, residual)` | Phase name rendered, an unknown one generically; the convergence chart only for `Iterate`; null `iter`/`residual` render no number; Cancel still calls `cancelSolve` | κ |
| BT30 | A realized plate solves (CLI) | Plate 300 × 50 × 5 mm with an r = 5 mm through-hole, `difference(box(300mm, 50mm, 5mm), cylinder_centered(5mm, 20mm))` (`box` is origin-centred; the adaptive-boundary-fidelity probe's own fixture), body overload | Auto: `converged = true`, Info's strategy consistent with its predicted bytes against the budget; `linear_solver: Iterative`: `iterations ≤ 100` (basis P3: 20–42 on R1–R5/T1; the leaf records the count) and max displacement within 1e-5 relative of the Auto run; a body of two boxes separated along the beam axis, `union(box(100mm, 20mm, 20mm), translate(box(100mm, 20mm, 20mm), 200mm, 0mm, 0mm))` (the root clamp at the overall x_min holds only the first; the tip load sits on the second), gives `FeaStiffnessNotPositiveDefinite` (unanchored), exit non-zero | μ |
| BT31 | A thin plate on its plate grid (CLI) | 800 × 500 × 12 mm on coordinate-target ζ's plate-capable grid, on three **fully restrained** non-collinear `PointSupport`s (all three directions; kinematically sufficient, and within #8252's closure — no directional restraint, #8256, is needed) | `converged = true`; Info names the strategy and DOFs; ζ's DOF-ceiling note records the solve under this PRD's solver | ν |
| BT32 | A load edit reuses the cached factor (engine) | One `Engine`: evaluate the smoke cantilever and the thin plate (Auto → Direct), change only the tip load's value, re-evaluate | The second solve's `FeaLinearSolveStrategy` says "factor reused (cached)"; its displacement is bitwise equal to a fresh `Engine`'s solve of the edited model; the same holds with `deterministic: true` (Q-H) | π |
| BT33 | A material edit reuses the symbolic analysis (engine) | As BT32, but change only the material's Young's modulus | The Info says "symbolic analysis reused (cached)" and a numeric factorisation ran (α's fault hook counts one numeric, zero symbolic calls); displacement bitwise equal to a fresh `Engine`'s solve | π |
| BT34 | Eviction bounds the cache (engine) | An engine whose warm-pool budget (`REIFY_WARM_STATE_BUDGET_BYTES` / config) is below the two models' combined factor bytes; evaluate model A, then B, then edit A's load and re-evaluate | `WarmStatePool::drain_events` reports an `Evicted` for the node whose entry the pool dropped; that node's next solve re-factors (its Info does not say "reused"); bits equal a fresh solve | π |

## 7. Decomposition plan

Prerequisites outside this batch: **#8244** (lands before β — Leo, esc-8244-3), **#7079** and
**#7080** (δ), **#7453** (η), **#6660** (μ), **#8252** (ν). `elastic_static.rs` is in the declared
files of #8244, #8246, #8248, #8254, #8266, #7781, #8257, #5313, #8078, the meshing-service leaves ε,
ζ, θ, λ, μ and the DWR leaves; **β is the only leaf of this batch that edits it** (S1), and it moves
code *out*.

**The cancel window before ι.** From β until ι lands, a cancel during a Direct solve takes effect at
the next phase boundary, so it waits for the whole inline symbolic or numeric factorisation in
progress: up to ≈ 45 s at the 2 GiB budget under this host's load (P2), longer for an explicit Direct
up to 8 GiB. The overlay is visible and Cancel is reachable throughout (β's phase-entry updates, C5);
only the latency is affected. ι closes the window.

| Leaf | Title | Modules | Depends on | Observable signal |
|---|---|---|---|---|
| **α** #8372 | `linear_solve` seam, Direct tier, policy; `SplitCholesky` driver shared with explicit `Par::Seq`; Jacobi-PCG as the interim iterative tier; zero-RHS rule; the `linear-solve-fault-injection` hook | reify-solver-elastic | — | Intermediate — unlocks β, γ, θ, ι. BT1–BT6, BT4b (seam). |
| **ε** #8373 | Pattern-first, node-owned assembly `assemble_stiffness` (`threads` + `ControlFlow` poll); `assemble_global_stiffness` becomes a wrapper over it, so no caller changes | reify-solver-elastic (`assembly/global.rs`) | — | Intermediate — unlocks β, ξ. BT7, BT7b, BT8. |
| **β** #8374 | Elastic static on the seam, every lane: adapter `static_linear_solve.rs` (which also owns the warm-state value, C13); `ElasticOptions.linear_solver` + `LinearSolver` (appended, pin 17 → 18); strategy Info; NPD (with `boundary::unanchored_components`), ceiling, residual and OOM codes; typed cancel; phase-entry progress updates in today's wire shape; every `elastic_static.rs` use of `resolve_execution_modes`/`SolverMode`/`PARALLEL_DOF_THRESHOLD` removed; stdlib doc rewrite (C6, C7); #8244's exhausted fixture selects `Iterative`; the GUI engine test's progress assertion updated | reify-solver-elastic (`boundary::unanchored_components`), reify-eval, reify-compiler stdlib, reify-cli tests, `gui/src-tauri/src/tests/engine_tests.rs` | α, ε, #8244 | BT9 (CLI), BT10–BT13e, BT12b, BT18 (Jacobi). Adds `linear_solver` to #7080's declaration if it has landed. |
| **ξ** #8375 | Retire the execution-mode API: delete `SolverMode::Parallel`, the parallel helpers, `PARALLEL_DOF_THRESHOLD`, `resolve_execution_modes`, `AssemblyMode` and ε's wrapper; move the remaining assembly callers to `assemble_stiffness`; rebuild `tests/determinism.rs` on the seam and `assemble_stiffness` | reify-solver-elastic (`solver.rs`, `lib.rs`, `assembly/`, `buckling_kernel.rs`, `shell_solve.rs`, `membrane_load.rs`, `tensegrity_load.rs`, tests), reify-eval (`modal_ops.rs`), reify-mesh-morph, reify-eval-fea-tests | β | Intermediate — unlocks γ (solver.rs) and θ. `tests/determinism.rs` bitwise across `threads ∈ {1, 4, 16}`. |
| **γ** #8376 | SA-AMG tier and the shared PCG loop; `solve_cg` re-expressed on it; `NearNullspace` from the adapter's coordinates; Auto over budget and `Iterative` become AMG; Q-I's work-budget calibration | reify-solver-elastic, reify-eval (adapter) | ξ | BT14 (engine), BT15–BT17c, BT18 (AMG). |
| **δ** #8377 | `max_iter` not applicable under Direct (`ParamNotApplicable`) | reify-eval (adapter) | β, #7079, #7080 | BT19, BT20 (CLI). |
| **ζ** #8378 | `solve_load_cases` factor reuse | reify-eval (`multi_case.rs`, adapter) | β | BT21 (CLI). Corrects `multi_case.rs`'s header ("re-homed to 4152"). |
| **η** #8381 | DWR dual through `solve_dual_cg(session, …)` (the dual call already sits in the adapter, C10); `SolverMode` deleted | reify-solver-elastic (`qoi.rs`, `solver.rs`, the `solve_cg` callers), reify-eval (adapter only) | β, γ, θ, #7453 | BT22 (engine). Amends goal-oriented §5.3's cost sentence (C10). |
| **θ** #8380 | Buckling pre-stress (P1, P2) and mesh-morph elasticity on the seam | reify-solver-elastic (`buckling_kernel.rs`), reify-eval (buckling trampoline), reify-mesh-morph, reify-audit's `engine_seam_g_allow_cites_live.rs` | γ (and ξ through it) | BT23 (CLI), BT24. |
| **ι** #8382 | Bounded-detach cancel (E′): `FactorLedger` (drop-guarded slot, memory admission), detach worker, `catch_unwind`, `Heartbeat`, `AwaitOrphan`, orphan debug event, `FeaSolverWorkerPanicked` | reify-solver-elastic, reify-eval (engine, adapter) | α, β | BT25–BT27 (engine). |
| **κ** #8383 | GUI: phase-shaped `solver-progress` wire (`phase`, optional `iter`/`residual`), overlay renders phases and unknown names generically, event-channel doc | reify-eval (`solver_progress.rs`), gui/src-tauri, gui/src, `docs/gui-event-channels/solver-progress.md` | β | BT28, BT29. |
| **λ** #8384 | Docs-truth: the FEA chunk's solver section (strategy, `linear_solver`, per-tier `max_iter`/`cg_tolerance`, determinism, the codes, the mechanism limit, cancel, the factor cache and its "reused (cached)" Info), `examples/best_practices/fea_solver_choice.ri` + `INDEX.md` row, one reify-design index line, discoverability | reify-mcp chunks, examples, `.claude/skills/reify-design/SKILL.md` | δ, ζ, θ, ι, κ, π | The chunk's fenced signatures pass the chunk fence gate; the exemplar compiles in `examples_smoke.rs` with its `INDEX.md` row and passes the best-practices constraint gate (allowlist pin with reason if its FEA constraint is INDETERMINATE); an author searching "my FEA solve is slow", "why didn't my solve converge" or "make the solver use iterations" lands on the chunk or index line. `chunks/fea.md` is extended if #8259, #7088 or meshing-service ν #8296 created it, else created (their rule). |
| **μ** #8385 | Integration gate: a realized plate with a hole from the CLI | reify-cli tests, fixtures | β, γ, #6660 | BT30 (CLI). |
| **ν** #8386 | Integration gate: the thin plate on coordinate-target's plate grid | reify-cli tests, fixtures | β, γ, #8252 | BT31 (CLI). |
| **π** #8379 | Numeric-factor cache across evaluations (C14): the adapter's warm-state value carries the factor and symbolic analysis keyed by `content_hash` + choice and `pattern_hash`; reuse Info; orphan factors never cached | reify-solver-elastic (`SpdSolver` built from a cached factor; `SpdSystem::pattern_hash`), reify-eval (adapter only) | β, ζ | BT32–BT34 (engine). GUI: the same Info in the diagnostics panel (no new GUI work). |
| **ω** #8387 | PRD close | this PRD, its manifest | α–ν, ξ, π | The terminal `Status` header, per the overlay's freeze shape. |

**DAG direction (every BT row against the leaf that produces what it needs).** α: BT1–BT6, BT4b
need only α (the fault hook is α's). ε: BT7, BT7b, BT8 need only ε (no caller changes). β: BT9–BT13e and BT12b need α, ε
and β (BT13d's hook is α's; BT13b's fixture is #8244's). ξ: needs β. γ: BT14–BT17c need ξ's cleaned
solver.rs and β's adapter (BT14's `with_solve_policy` is β's). θ also needs ξ (its files were moved to `assemble_stiffness` there). δ: BT19/BT20 need β's option and
#7079/#7080. ζ: BT21 needs β. η: BT22 needs β or #7453 (the dual in the adapter, C10), γ (session API on both tiers), θ (no remaining `solve_cg`
caller in buckling/morph) and #7453. θ: BT23's engine arm needs γ (AMG) and β (`with_solve_policy`); `BucklingOptions` is untouched. ι: BT25–BT27 need α's hook and
β's adapter. κ: BT28/BT29 need β's phase events (BT28's `AmgSetup` arm is BT14's). π: BT32–BT34 need β (the adapter owns the warm-state value, C13) and ζ
(`content_hash`). λ documents δ, ζ, θ, ι, κ, π. μ: needs β, γ (BT30's Iterative arm), #6660. ν: β, γ, #8252. No row's capability is
produced by a leaf downstream of it.

**D3 disposition (2026-10-08).** The D3 premise-verification run covered the six CLI leaves.
- **β, θ, ν: VERIFIED**, each with executed probes.
- **δ:** blocked only by a Prover fixture-path artefact (the probe could not locate its fixture). This
  is not a premise falsification, and no PRD change was made.
- **μ:** fixture-absent (the fixture is μ's own deliverable). The lead closed it with a direct probe:
  - `reify check` exits 0 on a debug build of 2026-10-07 for
    `difference(box(300mm, 50mm, 5mm), cylinder_centered(5mm, 20mm))`, and for the four-argument
    `translate(geo, dx, dy, dz)` form (a `vec3` argument is rejected). BT30 uses these spellings.
  - BT30 offsets the second box along x (`200mm, 0mm, 0mm`), not along y. A y offset leaves both boxes
    on the overall x_min face, so the root clamp would anchor both and the unanchored check would
    rightly stay silent.
- **ζ: one real finding.** On the dims path the supports list does not change the system. The
  adversary ran root-only, root + `PinnedSupport` at the tip, root + `FixedSupport` at the tip, and
  tip-only on the smoke cantilever. All four printed the same displacement (5.190735368751905e-4 m)
  and the same 719 iterations. So the original BT21 "case C differs in supports" would have shared
  A's factor. BT21 now differentiates C by `linear_solver`, and C10 says where support grouping is
  exercised.

**External edges and amendments at decompose:**
- **#8265** (coordinate-target η) depends on ν: its probe runs on this solver; its "if CG converges"
  wording becomes "if the static solve converges", and its GATE TEST's `deterministic: true` /
  parallel-threshold text is dropped (no threshold exists after ξ).
- **#8252** (coordinate-target ζ): its WORK item 3 and GATE TEST paragraph drop the same
  `PARALLEL_DOF_THRESHOLD` / "every coordinate-kind fixture sets `deterministic: true`" text; its DOF
  ceiling is measured on this PRD's solver if β has landed.
- **#8259**: its chunk line "every coordinate-kind fixture should set `deterministic: true` above the
  parallel threshold" is dropped.
- **#7052**: it **includes** `linear_solver` and `deterministic` in the options hash it makes present
  (C9; Q-H's in-process hole). `threads` may stay excluded, being bit-invisible after ε.
- **#8271** is closed by this batch (its scope is this PRD).
- **#7080**: if it lands after β, its C1 declaration includes `linear_solver` (honoured; union 18).
- **#7453**: if it lands after β, it writes its dual solve in the adapter through the primal's
  session (`SpdSolver::solve` on the homogeneous `g`), not as `solve_cg(…, SolverMode)` in
  `elastic_static.rs`; if it lands first, β moves its call (C10).
- **#2952**: its premise ("per-tick CG iterations materially lower with morphing") is re-stated
  against the AMG tier with `linear_solver: Iterative`, which keeps warm start; under Direct there are
  no iterations to compare.
- **#7836**: notes that morph's elasticity solve is on the seam (θ) and that "parallel assembly and
  CG" (mesh-morphing.md deferred option 2) is settled — assembly parallelism is bit-identical, CG is
  single-threaded; option 1 (factor caching across ticks) stays its decision.
- **#7088**: documents `max_iter`/`cg_tolerance` with C6's per-tier meanings.
- **Same-landing PRD prose edits** (this PRD's docs commit): coordinate-target-fea §5 C8's sentence on
  `PARALLEL_DOF_THRESHOLD`/`deterministic: true` and its §3 row on the thin plate; goal-oriented-error-
  estimation §2 "primal solve" and §5.3's cost sentence (pointing at η); structural-analysis-fea item
  12's "AMG preconditioner deferred" (resolved here); meshing-service §8/§9 "#8271" rows (point here).

**G6 notes.** BT9: today's failure measured (P1 probe); Direct's residual on every measured plate is
2e-12–1e-11, far under 1e-6 (P2). BT10: Q-B holds by construction (S6, C2 `Par::Seq`, sequential
AMG); P7 measured `Par::Seq` bit-identical across processes. BT12b's `residual = 1.0` is the identity
`‖f − K·0‖/‖f‖ = 1` for a cold solve with `f ≠ 0`. BT4: exact (pivots 1, −3), and run on both faer
variants because their index bases differ. BT4b/BT13e: the zero-RHS rule (S8) by construction.
BT15/BT30: bounds 1e-5 and 100 iterations sit 30× and 2.4× above the worst measured values (P5, P3)
and are recorded per leaf; a measured value over the bound is escalated, never widened. BT30's mesh
size is set by meshing-service's sizing, not the R-meshes, so its strategy assertion is a consistency
check against its own predicted bytes, not a claim about which tier wins. BT31's three fully restrained
point supports are kinematically sufficient, because the unanchored check cannot see a mechanism inside an anchored
component (C1). No signal asserts a time.

**G7 (reify invariants).** `diagnostics-carry-codes`: C8, every Error/Warning coded; the Info is
coded too. `error-severity-exits-nonzero`: every Stop except `Cancelled` is `Failed` with an Error.
`declared-intent-consumed-or-diagnosed`: `linear_solver` is read on every lane; `max_iter` on Direct
warns (δ); `threads` and `deterministic` keep honoured meanings (S6, Q-H). `declared-param-
reaches-kernel` (INV-PD-1): `linear_solver` joins #7080's honoured set. `result-fields-populated-or-
owned` (INV-PD-2): `iterations = 0` on Direct is a true count, documented (S8). `undef-has-
provenance`: no new `Undef`. `placeholders-owned-and-loud`: the interim Jacobi tier (S3) is replaced
by γ in the same batch; the cancel window before ι is named (above); the NodeCache staleness is owned
by #7052 (C9); the factor cache is a leaf (π) whose memory the pool's existing budget bounds. Umbrella: `NearNullspace` arrives with
γ, `AwaitOrphan`/`Heartbeat` with ι, the work term unset until γ calibrates it,
`ParamNotApplicable` use with δ — nothing is declared before it is read.

**Gate-test registration.** Each new integration-test binary (reify-solver-elastic `linear_solve_*`,
the reify-eval engine tests, the reify-cli harness modules) carries its nextest partition entry in the
same diff; BT8's comparison is a recorded one-off, not a test.

## 8. Out of scope

- **Shell, membrane and tensegrity solvers** stay on `solve_cg` (D-4); their tangent-K definiteness is
  unproven.
- **The modal and buckling eigensolves** (their own `SplitCholesky`/Lanczos paths), beyond C2's shared
  driver; reusing the buckling pre-stress factor inside the eigensolve.
- **Multithreaded factorisation** (faer's `rayon` feature) and any cross-machine bit pinning (Q-B).
- **Iterative refinement, IC(0), block Jacobi, nested-dissection orderings** (D-1, P4).
- **Factor caching across morph ticks** (#7836), **AMG-hierarchy caching**, and **a persisted factor**
  (C14).
- **Static P2 on the elastic path** (#7075): it consumes this seam unchanged; P2 fill is larger and the
  policy covers it.
- **The options cache keys** (#7052, #8140). **Meshing performance** (meshing-service D-G).
- **A memory budget for the AMG tier**: its footprint is O(nnz) (P3: 2.1 GB at 398k DOF); meshing-
  service θ's element budget bounds body meshes.
- **Detecting a mechanism inside an anchored component** (C1).

## 9. Cross-PRD relationship (G4)

| Other work | Direction | Seam mechanism | Owner |
|---|---|---|---|
| #8244 cap and options honesty | prerequisite | `max_iter`/`cg_tolerance` read on every lane; the cap-exhaustion code; the landed defaults | #8244 first; β adapts its exhausted fixture and reuses its code |
| `trampoline-param-drop-closure.md` #7079, #7080, #7088 | prerequisite (δ) / neighbour | `ParamNotApplicable`; the C1 declaration (18 params); the solver-options chunk section | those tasks; β or #7080 (second to land) adds `linear_solver`; δ emits the warning |
| `goal-oriented-error-estimation.md` #7453 | prerequisite (η) / modifies | `solve_dual_cg` and the primal's lifetime in `solve_cantilever_fea` | #7453 lands the dual; β or #7453 (second to land) puts it on the session in the adapter; η routes it through `solve_dual_cg` and edits §5.3 |
| `coordinate-target-fea.md` #8252, #8265, #8253 | prerequisite (ν) / produces for | the plate grid; the converging thin-plate solve #8265 needs; β's fixtures' `converged` | #8252 before ν; #8265 depends on ν; #8252/#8265/#8259 text amended; prose amended in this landing |
| `meshing-service.md` (#8288, #8289, #8291, #8296) | neighbour | morph's user path (BT11 there); `RealizedAdaptiveProblem` (unchanged here); element budget; the FEA chunk | that PRD; λ extends-or-creates `chunks/fea.md` |
| #6660 gmsh in author binaries | prerequisite (μ) | body solves from the CLI | #6660 |
| #7052 options in the keys | neighbour / limitation owner | `linear_solver` and `deterministic` in the options hash; the in-process NodeCache serving a stale tier on a GUI toggle, or a warm-started iterative result to a `deterministic: true` request (C9, Q-H) | #7052 (amended to include both) |
| #5215 / #7438 per-evaluation control | neighbour | the dispatch context carrying cancel; the ledger rides it | those tasks own the per-eval handle; ι adds the ledger beside it |
| #8266 convergence status | neighbour | `convergence_status: Converged` printed beside `converged = false` (P1 probe) | #8266 |
| `structural-analysis-fea.md` | modifies | item 12's deferred AMG | this PRD (prose in this landing) |
| `mesh-morphing.md`, #7836, #2952, #2953 | modifies / neighbour | morph's elasticity solve; warm-start premise; factor caching (π caches elastic static factors across evaluations; morph's across ticks stays #7836's) | θ the solve; π the elastic cache; #7836 caching across ticks; #2952 amended |
| #8246, #8248, #8254, #7781, #8257, #5313, #8078 | neighbour | `elastic_static.rs` hot file | serialise with β only; β shrinks the file |
| #7075 static P2 | consumer | the seam on P2 systems | #7075 |
| #4152 realization reuse | neighbour | `multi_case.rs`'s stale "re-homed to 4152" | ζ |
| #8271 bookmark | discharged | — | closed by this batch |

## 10. Open questions (tactical)

- **The detach heartbeat interval** (C5). Decide in ι.
- **`coarse_max`** (4,000 measured; 1,500 also measured on R2). Decide in γ from BT14–BT16 and
  record the per-level complexities in the Info's debug form.
- **Whether `SpdSystem::content_hash` hashes values with xxh3 over the raw `f64` bit arrays or reuses
  `ContentHash` helpers.** Decide in ζ.
- **The element-stiffness store's memory** (the caller's `Vec` of 12 × 12 blocks, 1.15 kB per P1 tet)
  — whether the adapter computes blocks on the fly during the gather instead. Decide in ε from a
  recorded peak.
- **`FeaLinearSolveStrategy`'s payload spelling.** Decide in β, matching `MeshModeUsed`'s style.
- **The best-practices exemplar's idiom** (an override for a known-huge model vs reading `iterations`
  and the strategy Info). Decide in λ.
- **Re-pointing the grammar fixture.** `tests/prd-gate/fixtures/elastic_solver_linear_solver_option.ri`
  uses a local stand-in enum and struct. Once β lands the stdlib `LinearSolver`, a same-named local enum
  may collide. Decide in β whether to re-point the fixture at the real `ElasticOptions` or rename the
  stand-in.
- **Whether `prepare` factors eagerly when the caller knows the RHS is non-zero** (C1's lazy rule
  defers to the first non-zero `solve`). Decide in α.
