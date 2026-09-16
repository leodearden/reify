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

## Hypothesis (NOT established fact) — RESOLVED by task 7411, see below

> **Status:** the hypothesis below was tested and CONFIRMED for the attributed
> call site, and both grounds it gave for deferring the fix were measured and
> overturned. Text preserved verbatim as the record of what was believed
> before the measurement; read
> "[Measured (task 7411)](#measured-task-7411--the-hypothesis-above-tested)"
> below for what is now established.

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

## Measured (task 7411) — the hypothesis above, tested

Same class of measurement as the 7414 log above, from a different task and lane:
warm lane `_lane-9`, task branch `task/7411` (= main @ `af103dcc53`), idle host,
`nproc=32`. Point-in-time, one host / one gmsh build, same caveat as §Measured.

Task 7411 was de-flaking a *different* e2e —
`reify-eval::morph_arm_e2e::e2e_non_structural_tick_morphs_and_preserves_connectivity`,
which intermittently recorded `remeshed_quality_soft_fail: 1` instead of
`morphed == 1` — but it runs through the same `kernel_real.rs`
`MeshingOptions::default()` seam this note fingered, so its measurements settle
the open questions here.

- **The CAUSE is confirmed, for the attributed call site.** No longer a
  hypothesis: driving `mesh_surface_to_volume_with_attribution` through the
  production trait method under `MeshingOptions::default()` produced **12
  distinct meshes in 12 runs** (tet counts 1163..1239, ±3%; verts 838..848).
  The `default()` literal at the call site IS the drift source.
- **`NumThreads = 1` alone IS stable — no `Mesh.RandomSeed` needed.** This
  retires the note's caveat that "NumThreads=1 HXT stability is itself
  unverified absent a `Mesh.RandomSeed`". With `deterministic: true` the same
  producer was **bit-identical across 12 runs spanning 2 processes** — at auto
  mesh size (verts=841 tets=1183) and at `mesh_size = 0.0015` (verts=1045
  tets=1531). Identity held ACROSS processes, not merely within one.
  `Mesh.RandomSeed` is still set nowhere in `crates/`, and is not needed.
- **The feared wallclock cost is INVERTED at realization mesh scale.** This
  retires the note's second deferral ground ("a real wallclock cost that needs
  its own benchmark"). At ~1200 tets a 32-thread HXT pool is pure overhead, so
  pinning is a large SPEEDUP:

  | Workload | `default()` | `deterministic: true` |
  |---|---|---|
  | morph e2e, 48 runs @ 8-way concurrency | 3m46s wall / 28m21s CPU | 10.4s wall / 22.5s CPU (~22x wall, ~76x CPU) |
  | gmsh attributed reproducibility test | 61–74s per run | 0.24–0.88s per run (~100x) |
  | morph e2e, single unloaded run | 4.1–12.4s | 0.30s |

  Caveat this does NOT cover: these are realization-scale meshes only. Whether
  the speedup survives at large GUI-realization mesh sizes is unmeasured, and
  is the open question for the remaining call site below.

### What remains unpinned after 7411

This note's remaining scope is now strictly narrower. Of the two
`kernel_real.rs` volume-meshing call sites its hypothesis named:

- the **attributed** override (`mesh_surface_to_volume_attributed`) is **pinned**
  by task 7411, guarded by
  `reify-kernel-gmsh/tests/mesh_surface_to_volume_attributed.rs::attributed_producer_output_is_reproducible_across_repeated_calls`
  (deterministically red without the pin);
- the **plain** override (`mesh_surface_to_volume`, `kernel_real.rs:582`) is
  **still unpinned**, and is now the file's only `MeshingOptions::default()`
  call site.

7411 stopped there deliberately: only the attributed branch produces morph
sources, and the plain one serves user-facing GUI realization rebuilds at mesh
sizes nobody has benchmarked — the one place the speedup above might not hold.
Tracked as ticket `tkt_0RTPM0DR78N6RG5Y7PZJNH2FZF` (which also carries a SPOT
item: the thread-count block is duplicated byte-for-byte at `kernel_real.rs:267-281`
and `mesh_boundary.rs:622-633`), alongside this note's existing
`tkt_0RTGVY62JW40ZMJDEQWEJRYCSE`.

Note the seed/refine asymmetry above is now half-closed: `RealizedAdaptiveProblem::new`
pins the REFINE step, 7411 pins the ATTRIBUTED seed producer, and only the plain
seed producer still drifts.
