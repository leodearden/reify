# Seed-mesh drift under the gmsh-realized adaptive lane — measurement log (task 7414)

Why this file exists: the e2e
`body_adaptive_solve_runs_the_gmsh_realized_localized_lane`
(`crates/reify-eval/tests/solve_elastic_static_body_e2e.rs`) used to assert a
terminal `BudgetReason` of `MaxIterations` specifically, and reddened the shared
merge-verify intermittently with `Stalled`. Task 7414 relocated that
discrimination to a deterministic unit pin
(`crates/reify-solver-elastic/tests/adaptive_refinement_tests.rs::stall_pre_empts_the_iteration_cap`).
The numbers below are what justified moving it rather than retuning it. They are
a **point-in-time log**, not a contract: they date to one host, one gmsh build
and one fixture, and nothing checks them.

## Provenance

Warm lane `_lane-29`, task branch `task/7414` (= main @ `a162d26572`), one test
binary built once, idle host, `nproc=32`.

## Measured

- **Flake rate, pre-fix.** 4 of 20 consecutive idle runs failed, every failure
  reporting `Stalled`. The task's analysis pass independently measured 3
  failures in ~50 runs, with the very first run failing. The task record also
  carries one merge-verify failure under fleet load.
- **The band.** With `run_adaptive_refinement` temporarily instrumented, 37 runs
  put the iteration-over-iteration indicator ratio `g1/g0` in **0.8147–0.9013**
  against an `is_stalled` threshold of **0.90**
  (`STALL_MIN_RELATIVE_DROP = 0.10`). The highest PASSING ratio was **0.8923**
  — 0.008 of headroom — and the single captured FAILURE sat at **0.9013**. So
  the `MaxIterations`-vs-`Stalled` discrimination was a ~1% numeric band on a
  noisy physical quantity, not a categorical fact.
- **Where the noise enters.** The seed tet count drifted **257–260** across runs
  at a CONSTANT **120 nodes / 360 dofs**. An iteration-0 indicator spread of
  ~0.34% amplified to ~8% by iteration 1 (the size-field chain is coarse and
  discontinuous, so a one-element change in the marked set rewrites the whole
  refined tetrahedralization). The post-refine peak is 1002 dofs, against the
  fixture's 2_000_000 `max_dofs` cap.
- **Flake rate, post-fix.** 25 of 25 consecutive runs passed in the same lane.

## Hypothesis (NOT established fact)

The SEED mesh is what is unpinned. `reify-kernel-gmsh`'s `kernel_real.rs` hands
`MeshingOptions::default()` to the 3D mesher at both volume-meshing call sites;
that default is `deterministic: false`, which resolves `General.NumThreads` to
`available_parallelism()` under `Mesh.Algorithm3D = 10` (HXT), and no
`Mesh.RandomSeed` is set anywhere in the repo. Those four are verified facts
about the code — that they are the CAUSE of the drift is the untested part.

Note the asymmetry that makes this plausible: the REFINE step is already pinned
(`RealizedAdaptiveProblem::new` sets `meshing_options.deterministic = true`,
documented there as load-bearing); only the seed is not.

Tracked separately as ticket `tkt_0RTGVY62JW40ZMJDEQWEJRYCSE` and deliberately
not fixed under task 7414 — pinning the seed is a cross-cutting change to every
`VolumeMesh` realization, with a real wallclock cost that needs its own
benchmark, and NumThreads=1 HXT stability is itself unverified absent a
`Mesh.RandomSeed`.
