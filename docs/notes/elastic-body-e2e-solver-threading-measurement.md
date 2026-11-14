# CG solver threading under the realized-cylinder body e2e — measurement log (task 9858)

## Provenance

Measured for task 9858 by its architect. Warm lane `_lane-0`, branch `task/9858`
(= main @ `2b1821797d`), 32-core host, loadavg ~100–270, release test binary.
These figures are point-in-time, from one host and one build.

`reify-eval`'s `solve_elastic_static_body_e2e.rs` B9 test,
`non_prismatic_two_case_build_realizes_body_exactly_once`, intermittently hit
its 1800s nextest timeout on a laptop verify host at loadavg 60–68. The test
runs two full OCCT+gmsh builds of a realized `cylinder(50mm, 200mm)`, which
means 2 gmsh volume meshes and 3 CG solves. Both stages ran host-width
threaded, and in both the wall time was dominated by cross-thread
synchronization latency, which host load amplifies without bound. The gmsh half
is logged in `adaptive-e2e-seed-mesh-drift-measurement.md`, § "What remained
unpinned after 7411 — now pinned (task 9858)". This note logs the solver half.

## Measured — the solve

The realized cylinder solves at `n_dofs` = 43,749, in 693 CG iterations. That is
above `PARALLEL_DOF_THRESHOLD` (10k). A bare `ElasticOptions()` leaves `threads`
at `None`, which resolves to `available_parallelism()`, so
`resolve_execution_modes` selects `SolverMode::Parallel{32}`. Its
`spmv_parallel` and `dot_parallel` each open a fresh `std::thread::scope` and
spawn 32 OS threads per call, several times per iteration.

| Solver mode | Wall time per solve |
|---|---|
| `Parallel{32}` (the default here) | 41–78s |
| `Parallel{4}` | 19–24s |
| `Deterministic` | 10.8–12.2s |

The whole B9 test, all four arms at the same moment:

| gmsh producer | Solver mode | Wall | Notes |
|---|---|---|---|
| 32 threads (`default()`) | `Parallel{32}` | 240s | user 208s, sys 30s, 241k voluntary context switches |
| 1 thread (pinned) | `Parallel{32}` | 192s | |
| 1 thread (pinned) | `Parallel{4}` | 71s | |
| 1 thread (pinned) | `Deterministic` | 41s | user 27s, sys 0.6s, 186 voluntary context switches |

The serial arm is plain CPU work, whose wall time scales roughly linearly with
the CPU share it gets. The parallel arms are synchronization-bound.

With both pins in place (task 9858's branch, release, loadavg 95 → 277), the B9
test ran in 44.5s.

## What the fixtures pin, and what production will do

The three cylinder fixtures pass `ElasticOptions(deterministic: true)`. That
selects the `Deterministic` execution mode: a single-threaded CG and assembly,
and a cold start that discards any CG warm state (the `warm_start` arm of
`reify-eval`'s `compute_targets/elastic_static.rs`). None of those tests
exercise warm start. The box fixtures realize at ~360 DOFs, below the
threshold, so they already took the `Deterministic` path.

`docs/prds/v0_6/elastic-static-solver-performance.md` makes every solve
single-threaded and deletes the parallel mode. That PRD's leaves own the
production execution-mode selection.
