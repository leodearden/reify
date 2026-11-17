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

## Post-fix per-test cost, and the retired 1800s override (task 9910)

### Provenance

Measured 2026-11-14 by task 9910's architect, on two hosts:

- Main host, 32 cores. Warm lane `_lane-0`, main @ `481da661fe`, which contains
  9858's merge `05b99dda8d`. Command:
  `cargo nextest run [--release] -p reify-eval --test solve_elastic_static_body_e2e`
  with the `scripts/gen-nextest-config.sh` config.
- Laptop, 16 cores. Prebuilt test binaries from its `_merge-verify` lane @
  `5a51d21ca9`, a descendant of `05b99dda8d`. One process per test, run through
  `.cargo/run-with-occt.sh`.

Like the figures above, these are point-in-time.

### Measured

| Host | Profile | loadavg | B9 wall | Next-slowest |
|---|---|---|---|---|
| main | debug | 57–83 | 24.2s | 16.0s |
| main | debug | 153–179 | 35.1s | 22.9s |
| main | release | 102–179 | 34.7s | 23.5s |
| laptop | release | 7–25 | 14.1s | 9.5s |
| laptop | debug | 7–25 | 14.9s | 9.8s |

The worst post-fix figure on record is not in this table. It is the 44.5s
release run with both pins above (loadavg 95 → 277).

Every run passed 9/9. B9 (`non_prismatic_two_case_build_realizes_body_exactly_once`)
was the slowest test in every run. The box-fixture tests took 0.2–3.6s.

Gate evidence: the laptop merge verifies of `05b99dda8d` and `481da661fe` both
passed full debug and release nextest passes. No timeout of this binary has
been recorded since 9858.

### Why first-hand runs, not gate logs

`data/verify-logs/` keeps a per-test nextest log only for failing runs. On
2026-11-14 the main host held 188 `.log` files against 1157 summary JSONs, and
neither host held any `.log` file from after 9858. PASS times for a binary that
keeps passing never build up there.

### Pre-fix contrast

From the logs that remain: the worst PASS was 548.7s on the main host and 168.4s
on the laptop (debug). Tasks 6525, 9756 and 9099 recorded 1800s TIMEOUTs, 9099
twice.

### Decision

The worst post-fix figure is 44.5s against the 1200s default, about 27x. The
false-kill floor is 3x. So `.config/nextest.toml` dropped task 7339's 1800s
override, and the binary now inherits `[profile.default]`. A 1200s timeout of
this binary is a regression to escalate with its `data/verify-logs` path. It is
not a cue to re-add an override.
