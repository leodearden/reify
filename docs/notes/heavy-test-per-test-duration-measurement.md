# Heavy-test per-test duration measurement (task 7552)

RAW EVIDENCE ONLY. This file records what was observed, so the next person to
re-tune the heavy per-test ceiling can do it from data instead of re-measuring
blind. It deliberately carries no derived figure: the ceiling, its derivation and
the constraints it satisfies live in `docs/prds/offline-deep-test-lane.md` DA6,
which cites this file for the evidence. One home per number
(`docs/code-quality.md` heuristic 11).

Same split as `docs/notes/multi-process-occt-bench.md`, which `.config/nextest.toml`
already cites for the occt cap's headroom basis: summary and derivation in the
consuming file, full evidence here.

## Why this measurement existed

DA6 conceded in its own closing paragraph that the 12h (43200s) heavy ceiling
then in force "is not a measured figure", and named measurement as the
prerequisite to settle BEFORE the `DF_VERIFY_ROLE=background` reachability gap.
The two `#[ignore]`d convergence studies in
`crates/reify-solver-elastic/tests/analytical_validation.rs` were called out by
name as the genuinely unknown cost: they run on NO path except the offline lane's
`--run-ignored all` pass, so their duration had never been observed at all.

## Sample size — read this before using the numbers

**N = 2 runs.** Two samples are not a distribution. They bound the worst case
observed on ONE host under the contention that happened to be present, and
nothing more: no percentile, no variance, no claim about a cold-cache or
differently-loaded host. The spread between the two runs is already large (the
slowest test more than doubled between them, 383.3s to 545.0s, tracking the host
load) which is itself the main finding — the dominant term is contention, not
intrinsic cost. Treat the max column as a floor on what a loaded host can
produce, not a ceiling on it.

## What was run

- **HEAD:** `e49c9ee21884664f2c9fe9c92250f0c24831b533` (task/7552, at main tip)
- **Worktree:** `/home/leo/src/warm-lanes/worktrees/_lane-3`
- **Host:** `nproc` = 32, 125 GiB RAM
- **Native deps preflight:** `scripts/check-manifold-deps.sh` exit 0 — OCCT 7.8
  (`/usr/lib/x86_64-linux-gnu`), Gmsh 4.15.2 and OpenVDB 13.0 (`/opt/reify-deps/lib`)
  all present. Run FIRST and recorded, per CLAUDE.md's SILENT-VACUITY RULE: a
  missing native dep makes the heavy surface stop COMPILING and the run reports
  zero tests rather than zero failures, which would look like a fast measurement.

The command was not hand-rolled. It was lifted verbatim from the release
`cargo nextest run` line of

```
DF_VERIFY_ROLE=offline bash scripts/verify.sh test --scope all --print-plan
```

so the thing measured is the invocation the offline lane really issues, including
its positive heavy filterset and `--run-ignored all`:

```
nice -n 19 ionice -c3 cargo nextest run --workspace --release \
  -E "((package(reify-solver-elastic) & binary(determinism)) | (package(reify-solver-elastic) & binary(analytical_validation)) | (package(reify-solver-elastic) & binary(modal_benchmarks)) | (package(reify-eval-fea-tests) & binary(buckling_smoke)) | (package(reify-eval) & binary(tensegrity_t0a)) | (package(reify-eval-fea-tests) & binary(fea_diagnostics_e2e)) | (package(reify-eval) & binary(harness_fea_solver_e2e) & test(/^fea_in_the_loop_producer::/)) | (package(reify-eval) & binary(harness_fea_solver_e2e) & test(/^fea_bracket_minimize_mass_e2e::/)))" \
  --run-ignored all --config-file "$(bash scripts/gen-nextest-config.sh)"
```

Environment as the plan renders it: `RUSTC_WRAPPER=sccache`,
`CARGO_INCREMENTAL=0`, `LD_LIBRARY_PATH=/opt/reify-deps/lib`.

Two method notes, both load-bearing:

- **The release test binaries were built ONCE beforehand**, by the same command
  with `--no-run` appended (9m 40s, exit 0, no `cargo:warning` about a missing
  native dep). Compile time is therefore NOT folded into any per-test duration
  below — but it is also not included in the whole-run wall-clock, which is why
  those figures are a floor and not the lane's real cost. See the caveat under
  "Whole-run wall-clock".
- **Output went to a log FILE, and the file was parsed.** The repo's PreToolUse
  skim wrapper condenses `cargo test`/`cargo build` output to
  `PASS: N | FAIL: M | SKIP: K`, which would have destroyed the per-test
  durations that are this measurement's entire deliverable. Invoked via an
  absolute cargo path so the condensation never applied.

## Host state per run

Both runs ran back to back under the host's NATURAL contention. The host was
deliberately not quiesced: both roles that run the heavy set (`offline` and
`background`) run contended, so a quiet-box figure would understate the number
the ceiling has to cover.

| | run 1 | run 2 |
|---|---|---|
| start | 2026-09-22T07:47:03+01:00 | 2026-09-22T07:59:53+01:00 |
| end | 2026-09-22T07:59:53+01:00 | 2026-09-22T08:11:04+01:00 |
| `/proc/loadavg` at start | 57.88 111.48 129.76 | 105.12 103.04 120.38 |
| `/proc/loadavg` at end | 105.12 103.04 120.38 | 130.93 122.70 121.63 |
| exit code | 0 | 0 |
| nextest summary | `52 tests run: 52 passed (6 slow), 163 skipped` | `52 tests run: 52 passed (6 slow), 163 skipped` |
| `SLOW [>Ns]` progress markers emitted | 8 | 12 |

Other verify runs were active on the host throughout (this is a shared 32-core
box running the orchestrator's lane pool); run 2 executed at roughly double run
1's load average, which is where its larger numbers come from.

## Whole-run wall-clock

| | run 1 | run 2 |
|---|---:|---:|
| nextest-reported execution | 384.2 s | 545.8 s |
| driver-measured wall-clock (incl. nextest startup, binary list, target-runner) | 770 s | 671 s |

**CAVEAT, do not quote these as the offline lane's cost.** Both ran against a
PRE-BUILT `target/`. The offline lane pays a release build — including the cold
sccache native-kernel cone — before this pass. These figures bound the EXECUTION
half only. The lane's recorded whole-run sub-runs reach 2625s; these numbers are
a floor under that, not a replacement for it.

## The two convergence studies

DA6's named unknown, and PRD §10's open question. Both are `#[ignore]`d in
`crates/reify-solver-elastic/tests/analytical_validation.rs` with
`reason = "convergence study; ..."`, and `--run-ignored all` is what selects
them. **This is their first first-class execution on any path.**

| Test | run 1 | run 2 | result |
|---|---:|---:|---|
| `cantilever_faithful_convergence_study` | 44.5 s | 32.1 s | PASS both runs |
| `cylinder_lame_convergence_study` | 2.8 s | 2.5 s | PASS both runs |

Both PASS. Neither is anywhere near the slow end of the set — the cantilever
study is 17th of 52 by cost and the cylinder study 32nd. PRD §10 anticipated that
a first first-class run might RED-light; it did not.

## Observed failures and timeouts

**None.** 52 of 52 tests passed in both runs. No `FAIL`, no `TIMEOUT`, no
retry, no leak. The `(6 slow)` in each summary counts tests that crossed a
`SLOW [>Ns]` progress threshold, which is a nextest progress marker and not a
failure or a kill.

## Per-test durations

**RELEASE PROFILE ONLY.** The heavy ceiling governs both profiles; the debug
figures, and the pre-test-start overhead this section says nothing about, are in
"Pre-test-start overhead and debug-profile cost — task 7552 amendment" below.

52 tests, one row per test the heavy filterset selected, one column per run,
plus the per-test max. Sorted by max, descending.

| Test (`package::binary testname`) | run 1 (s) | run 2 (s) | max (s) |
|---|---:|---:|---:|
| `reify-eval::harness_fea_solver_e2e fea_in_the_loop_producer::solve_elastic_static_dispatches_real_result_inside_minimize_where_loop` | 383.3 | 545.0 | **545.0** |
| `reify-solver-elastic::determinism default_parallel_tolerance_equivalent_across_repeated_runs` | 219.2 | 389.5 | **389.5** |
| `reify-solver-elastic::determinism default_parallel_tolerance_equivalent_across_thread_counts` | 222.9 | 350.1 | **350.1** |
| `reify-solver-elastic::determinism deterministic_stress_field_and_von_mises_bit_stable_across_thread_counts` | 145.9 | 186.2 | **186.2** |
| `reify-solver-elastic::determinism deterministic_displacement_bit_stable_across_repeats_and_thread_counts` | 140.6 | 170.0 | **170.0** |
| `reify-eval::harness_fea_solver_e2e fea_bracket_minimize_mass_e2e::fea_bracket_minimize_mass_example_converges_to_an_interior_thickness` | 127.0 | 139.1 | **139.1** |
| `reify-eval-fea-tests::buckling_smoke e2e_buckling_smoke_lowers_to_compute_node` | 92.7 | 106.0 | **106.0** |
| `reify-solver-elastic::analytical_validation thick_walled_cylinder_p2_max_von_mises_within_2pct_of_lame` | 99.6 | 94.5 | **99.6** |
| `reify-solver-elastic::modal_benchmarks clamped_clamped_beam_p2_modal_within_calibrated_band` | 83.0 | 98.2 | **98.2** |
| `reify-eval-fea-tests::buckling_smoke e2e_buckling_critical_load_within_ten_percent` | 96.1 | 85.6 | **96.1** |
| `reify-solver-elastic::analytical_validation cantilever_beam_p2_tip_deflection_slender_within_1pct_of_timoshenko` | 95.6 | 89.7 | **95.6** |
| `reify-eval-fea-tests::buckling_smoke e2e_buckling_second_eval_hits_cache` | 94.8 | 87.6 | **94.8** |
| `reify-eval-fea-tests::buckling_smoke e2e_buckling_pre_stress_displacement_stress_fields` | 89.9 | 77.7 | **89.9** |
| `reify-solver-elastic::analytical_validation boussinesq_subsurface_sigma_z_p2_within_10pct` | 68.2 | 83.4 | **83.4** |
| `reify-solver-elastic::modal_benchmarks simply_supported_beam_p2_modal_within_two_percent` | 73.9 | 61.1 | **73.9** |
| `reify-solver-elastic::modal_benchmarks cantilever_beam_p2_modal_within_two_percent` | 50.9 | 48.8 | **50.9** |
| `reify-solver-elastic::analytical_validation cantilever_faithful_convergence_study` | 44.5 | 32.1 | **44.5** |
| `reify-solver-elastic::analytical_validation boussinesq_subsurface_sigma_z_p1_within_10pct` | 25.2 | 28.7 | **28.7** |
| `reify-solver-elastic::analytical_validation cantilever_beam_p2_tip_deflection_within_3pct_of_timoshenko` | 20.6 | 23.7 | **23.7** |
| `reify-solver-elastic::analytical_validation cantilever_beam_p1_tip_deflection_within_5pct_of_timoshenko` | 13.3 | 7.5 | **13.3** |
| `reify-eval-fea-tests::fea_diagnostics_e2e under_constrained_present_but_unhonored_support_emits_labeled_warning` | 6.3 | 1.1 | **6.3** |
| `reify-solver-elastic::analytical_validation cantilever_hex_converges_steeper_than_tet_at_equal_dof` | 2.4 | 5.6 | **5.6** |
| `reify-solver-elastic::analytical_validation cylinder_hex_converges_steeper_than_tet_at_equal_dof` | 5.4 | 1.9 | **5.4** |
| `reify-solver-elastic::analytical_validation thick_walled_cylinder_hex_p1_max_von_mises_within_5pct_of_lame` | 4.8 | 0.7 | **4.8** |
| `reify-eval-fea-tests::fea_diagnostics_e2e thin_body_fixture_emits_fea_thin_body_warning` | 4.8 | 2.8 | **4.8** |
| `reify-solver-elastic::analytical_validation thick_walled_cylinder_p1_max_von_mises_within_5pct_of_lame` | 4.3 | 2.2 | **4.3** |
| `reify-eval-fea-tests::fea_diagnostics_e2e no_loads_fixture_emits_fea_no_loads_warning` | 3.5 | 4.0 | **4.0** |
| `reify-eval::tensegrity_t0a strut_ctor_evaluates_to_structure_instance` | 0.9 | 3.2 | **3.2** |
| `reify-eval::tensegrity_t0a tensegrity_ctor_carries_node_and_index_lists` | 3.2 | 0.8 | **3.2** |
| `reify-eval::tensegrity_t0a cable_ctor_evaluates_to_structure_instance_with_pretension_default` | 2.7 | 2.8 | **2.8** |
| `reify-solver-elastic::analytical_validation cantilever_beam_hex_p1_tip_deflection_within_5pct_of_timoshenko` | 2.8 | 0.8 | **2.8** |
| `reify-solver-elastic::analytical_validation cylinder_lame_convergence_study` | 2.8 | 2.5 | **2.8** |
| `reify-eval-fea-tests::fea_diagnostics_e2e cantilever_smoke_does_not_emit_fea_thin_body` | 1.8 | 2.3 | **2.3** |
| `reify-solver-elastic::analytical_validation element_stress_hex_p1_uniaxial_strain_patch_recovers_lame_diagonal` | 1.6 | 2.3 | **2.3** |
| `reify-solver-elastic::analytical_validation simple_shear_uniform_stress_p1_within_1pct` | 2.2 | 0.7 | **2.2** |
| `reify-solver-elastic::analytical_validation cantilever_beam_wedge_p1_tip_deflection_within_tol_of_timoshenko` | 1.9 | 1.2 | **1.9** |
| `reify-solver-elastic::determinism deterministic_fast_shared_dof_cantilever_full_pipeline_bit_stable` | 1.9 | 1.8 | **1.9** |
| `reify-eval::tensegrity_t0a tensegrity_wires_undef_on_out_of_range_index` | 0.7 | 1.7 | **1.7** |
| `reify-solver-elastic::analytical_validation simple_shear_uniform_stress_p2_within_1pct` | 1.6 | 1.0 | **1.6** |
| `reify-solver-elastic::analytical_validation lame_reference_satisfies_known_invariants` | 1.3 | 0.4 | **1.3** |
| `reify-eval-fea-tests::buckling_smoke register_compute_fns_installs_solver_buckling` | 1.3 | 1.1 | **1.3** |
| `reify-eval-fea-tests::fea_diagnostics_e2e no_supports_fixture_emits_fea_under_constrained_warning` | 1.1 | 0.9 | **1.1** |
| `reify-eval::tensegrity_t0a membrane_ctor_evaluates_to_structure_instance_with_prestress_default` | 0.9 | 0.7 | **0.9** |
| `reify-eval::tensegrity_t0a tensegrity_wires_undef_on_wrong_type_name` | 0.8 | 0.3 | **0.8** |
| `reify-eval::tensegrity_t0a tensegrity_wires_undef_on_real_arg` | 0.8 | 0.3 | **0.8** |
| `reify-eval::tensegrity_t0a tensegrity_wires_preserves_declaration_order_struts_then_cables` | 0.7 | 0.4 | **0.7** |
| `reify-eval::tensegrity_t0a tensegrity_ctor_without_surfaces_evals_without_surfaces_field` | 0.6 | 0.7 | **0.7** |
| `reify-solver-elastic::analytical_validation annular_polar_mesh_is_valid` | 0.6 | 0.2 | **0.6** |
| `reify-eval::tensegrity_t0a tensegrity_surfaces_emits_two_tagged_facets` | 0.6 | 0.3 | **0.6** |
| `reify-eval::tensegrity_t0a tensegrity_wires_emits_six_tagged_wires` | 0.5 | 0.4 | **0.5** |
| `reify-eval::tensegrity_t0a tensegrity_wires_undef_on_two_args` | 0.5 | 0.5 | **0.5** |
| `reify-eval::tensegrity_t0a tensegrity_wires_undef_on_zero_args` | 0.5 | 0.1 | **0.5** |

**Global per-test max across both runs: 545.0 s** —
`reify-eval::harness_fea_solver_e2e fea_in_the_loop_producer::solve_elastic_static_dispatches_real_result_inside_minimize_where_loop`,
in run 2.

## Shape of the distribution, stated plainly

The set is heavily skewed: 6 tests account for essentially all of the cost (the
top 6 maxima sum to ~1780 s of a ~545 s critical path once parallelism is
accounted for), and 29 of the 52 complete in under 5 s. The binaries
`tensegrity_t0a` and `fea_diagnostics_e2e` are in the heavy filterset for
membership reasons, not cost — every one of their tests is under 7 s in both
runs. A future re-tune should not read "heavy filterset member" as "slow test".

## Pre-test-start overhead and debug-profile cost — task 7552 amendment

Everything above was measured on the OFFLINE lane's RELEASE invocation. That
left two quantities unmeasured which the ceiling nonetheless depends on, and
this section records them. Same discipline: observations only, no derived
figure — the ceiling and its derivation stay in DA6.

### Why a second measurement was needed

1. **Pre-test-start overhead (O).** `scripts/verify.sh` wraps each nextest pass
   in `timeout --kill-after=60 <wall> ... cargo nextest run ...` around what its
   own comment calls "one combined build+execution nextest pass per profile", so
   that wall's clock starts at PASS start and includes the build. nextest's
   `slow-timeout.terminate-after` clock starts at TEST-PROCESS start. The two
   clocks do not start together, and nothing above measured the gap.
2. **Debug-profile per-test cost (M_debug).** `[profile.default]`'s overrides
   govern BOTH profiles, and `DF_VERIFY_ROLE=background` forces
   `--profile both`. Every per-test figure above is a RELEASE figure.

### What was run

The debug pass of the `background` role, lifted verbatim from

```
DF_VERIFY_ROLE=background bash scripts/verify.sh test --scope all --print-plan
```

which renders it as

```
timeout --kill-after=60 60m nice -n 19 ionice -c3 \
  cargo nextest run --workspace --config-file <gen-nextest-config.sh output> 9<&-
```

No `--release`, no `-E` filter, no `--run-ignored` — this pass runs the whole
workspace and does NOT run the two convergence studies. Environment as the plan
renders it: `RUSTC_WRAPPER=sccache`, `CARGO_INCREMENTAL=0`,
`LD_LIBRARY_PATH=/opt/reify-deps/lib`. `CARGO_MAKEFLAGS` was not set (the
jobserver FIFO the plan names belongs to a verify.sh process, not to this one).

- **HEAD:** `2c67cf22b82d92066fa73161954cb077a199ee60` (task/7552)
- **Native deps preflight:** `scripts/check-manifold-deps.sh` exit 0 — OCCT 7.8
  (`/usr/lib/x86_64-linux-gnu`), Gmsh 4.15.2, OpenVDB 13.0
  (`/opt/reify-deps/lib`). Run FIRST, per CLAUDE.md's SILENT-VACUITY RULE.
- **Timestamper:** `ts` (moreutils) is NOT installed on this host, so the
  fallback was used: `gawk '@load "time"; { printf "%.3f %s\n", gettimeofday(),
  $0; fflush() }'`. `gettimeofday()` gives MILLISECOND resolution, so no figure
  here is quantised by the timestamper. (`systime()` would have quantised every
  one of them to a whole second; it was not used.) Output went to a log FILE and
  the file was parsed, via an absolute cargo path, so the PreToolUse skim
  wrapper's `PASS: N | FAIL: M` condensation never applied.
- **Start offsets are DERIVED, not printed.** nextest prints no start time. For
  every completion line `PASS [ <d>s] <binary> <test>` the start was computed as
  `line_timestamp - t0 - d`, where `t0` is the wall-clock instant the
  `cargo nextest run` process was launched.

### Host state per run

Both runs back to back, host not quiesced. Other verify runs were active
throughout; run 1 began at loadavg 254 on 32 cores.

| | run 1 | run 2 |
|---|---|---|
| start | 2026-09-22T09:41:55+01:00 | 2026-09-22T10:11:19+01:00 |
| end | 2026-09-22T10:11:19+01:00 | 2026-09-22T10:24:33+01:00 |
| `/proc/loadavg` at start | 254.08 153.67 135.85 | 82.36 161.72 176.10 |
| `/proc/loadavg` at end | 82.36 161.72 176.10 | 121.90 186.44 179.68 |
| `target/` state | WARM but stale — `target/debug/deps` already held 17607 files from earlier lane work at an older tree | WARM from run 1 — 17912 files, nothing to rebuild |
| exit code | 0 | 0 |
| driver-measured wall-clock | 1763.4 s | 794.6 s |
| nextest summary | `24690 tests run: 24690 passed (18 slow, 22 leaky), 70 skipped` | `24690 tests run: 24690 passed (11 slow, 4 leaky), 70 skipped` |
| nextest-reported execution | 1218.997 s | 778.070 s |
| sccache compile requests (delta) | +1503 | +566 |
| sccache requests EXECUTED (delta) | +201 | +11 |
| sccache cache hits / misses (delta) | +161 / +40 | +2 / +9 |

**READ THE CACHE ROW BEFORE ANY OTHER NUMBER HERE.** Cache and `target/` state
is the dominant term in O and these figures are worthless without it. Run 1
executed only 201 compilations: its 544 s of pre-test time was overwhelmingly
LINKING — 623 test binaries — plus nextest's own binary-list pass, not rustc
work sccache could have absorbed. Neither run measured a COLD `target/`, and
neither is an upper bound on one. `scripts/verify.sh`'s own recorded 798.9 s
worst-observed healthy whole-pass debug completion was measured on a cold target
with a warm sccache, i.e. the state neither run here reproduces.

### QUANTITY 1 — pre-test-start overhead (O)

Per-atom first-start offsets, seconds after the `cargo nextest run` launch:

| heavy atom | run 1 | run 2 |
|---|---:|---:|
| `reify-eval::tensegrity_t0a` (LPT priority 100) | +544.5 | +16.5 |
| `reify-eval-fea-tests::fea_diagnostics_e2e` (priority 50) | +544.6 | +16.5 |
| `reify-solver-elastic::analytical_validation` (priority 50) | +544.6 | +16.5 |
| `reify-solver-elastic::determinism` (priority 50) | +546.6 | +17.8 |
| `reify-eval-fea-tests::buckling_smoke` (no priority) | +904.8 | +218.6 |
| `reify-eval::harness_fea_solver_e2e [fea_bracket_minimize_mass_e2e::*]` (no priority) | +1078.6 | +315.3 |
| `reify-eval::harness_fea_solver_e2e [fea_in_the_loop_producer::*]` (no priority) | +1078.7 | +315.5 |
| `reify-solver-elastic::modal_benchmarks` | not run — see below | not run |

| | run 1 | run 2 |
|---|---:|---:|
| `O_first` — first test of ANY kind to start | +544.5 s | +16.5 s |
| `O_heavy_max` — LATEST-starting heavy atom | +1078.7 s | +315.5 s |

**`O_bound` = 1078.7 s**, the max of `O_heavy_max` across the two runs. It is a
max over N = 2 observations on one host, and it is a bound in no stronger sense
than that. Its two terms move independently and neither was measured at its
worst: the build term (544.5 s vs 16.5 s here) tracks `target/`/sccache state,
and the queue term — 534.2 s in run 1, 299.0 s in run 2 — tracks host
contention and the test-thread pool.

Two observations worth separating from the numbers:

- **The LPT priority overrides do not cover the whole heavy set, and the gap is
  large.** The four atoms carrying `priority = 100`/`50` start within ~2 s of
  the first test, as intended. The three that carry no priority start 360 s and
  534 s later (run 1). `O_heavy_max` is therefore a scheduling fact about the
  un-prioritised members, not a build fact — which is why it has to be measured
  rather than assumed equal to `O_first`.
- **In run 1 the latest heavy start (+1078.7 s) was ~2x `O_first` (+544.5 s).**
  Quoting the build term alone would understate the offset by half.

### QUANTITY 2 — debug-profile per-test cost (M_debug)

43 heavy tests ran in the debug profile across 7 of the 8 atoms. All passed in
both runs. No `FAIL`, no `TIMEOUT`, no kill.

**`reify-solver-elastic::modal_benchmarks` does not run in this profile at all.**
Its tests carry `cfg_attr(debug_assertions, ignore)`
(`crates/reify-solver-elastic/tests/modal_benchmarks.rs`, task 4066: "release-only
at the merge gate; debug skips it for per-task speed"), and this pass passes no
`--run-ignored`. Its per-test costs are therefore release-only figures and stay
in the release table above.

Debug per-test durations, slowest first. This table is a TRUNCATION, not a full
listing: 15 of the 43 rows are shown and the other 28 — every test whose max is
below 3.0 s — are omitted. Every omitted one is a `tensegrity_t0a`,
`fea_diagnostics_e2e`, `buckling_smoke` or sub-second `analytical_validation`
test, and the full list is recoverable from the same logs. (15 + 28 = 43; an
earlier caption said 30 omitted and claimed one row per heavy test, which no
reader could reconcile against the 43 above it.)

| Test (`atom testname`) | run 1 (s) | run 2 (s) | max (s) |
|---|---:|---:|---:|
| `harness_fea_solver_e2e fea_in_the_loop_producer::solve_elastic_static_dispatches_real_result_inside_minimize_where_loop` | 545.2 | 398.5 | **545.2** |
| `determinism default_parallel_tolerance_equivalent_across_thread_counts` | 526.2 | 270.7 | **526.2** |
| `determinism default_parallel_tolerance_equivalent_across_repeated_runs` | 520.4 | 284.9 | **520.4** |
| `determinism deterministic_stress_field_and_von_mises_bit_stable_across_thread_counts` | 166.6 | 113.3 | **166.6** |
| `harness_fea_solver_e2e fea_bracket_minimize_mass_e2e::fea_bracket_minimize_mass_example_converges_to_an_interior_thickness` | 160.8 | 139.5 | **160.8** |
| `determinism deterministic_displacement_bit_stable_across_repeats_and_thread_counts` | 138.2 | 110.3 | **138.2** |
| `analytical_validation thick_walled_cylinder_p2_max_von_mises_within_2pct_of_lame` | 102.4 | 62.8 | **102.4** |
| `analytical_validation cantilever_beam_p2_tip_deflection_slender_within_1pct_of_timoshenko` | 83.5 | 73.0 | **83.5** |
| `analytical_validation boussinesq_subsurface_sigma_z_p2_within_10pct` | 70.8 | 43.1 | **70.8** |
| `analytical_validation cantilever_beam_p2_tip_deflection_within_3pct_of_timoshenko` | 24.1 | 13.2 | **24.1** |
| `analytical_validation boussinesq_subsurface_sigma_z_p1_within_10pct` | 19.7 | 11.0 | **19.7** |
| `analytical_validation cantilever_beam_p1_tip_deflection_within_5pct_of_timoshenko` | 7.7 | 5.6 | **7.7** |
| `fea_diagnostics_e2e thin_body_fixture_emits_fea_thin_body_warning` | 4.8 | 4.0 | **4.8** |
| `fea_diagnostics_e2e no_supports_fixture_emits_fea_under_constrained_warning` | 3.1 | 1.0 | **3.1** |
| `determinism deterministic_fast_shared_dof_cantilever_full_pipeline_bit_stable` | 2.8 | 3.1 | **3.1** |

**`M_debug` = 545.2 s** — `fea_in_the_loop_producer::solve_elastic_static_dispatches_real_result_inside_minimize_where_loop`,
in run 1.

**The same test is the pole in both profiles, at the same cost.** Its release max
was 545.0 s (table above); its debug max is 545.2 s. No heavy member costs
materially more in debug than in release on this host, so the debug profile does
not move the worst case. That is an observation about these two runs, not a
general claim: the next three rows all more than doubled between run 1 and run 2
purely on load, so contention dominates the profile difference in both
directions.

### Observed failures and timeouts

**None**, in either run. 24690 of 24690 tests passed in each, exit code 0 both
times. The `(18 slow)` / `(11 slow)` counts are `SLOW [>Ns]` progress markers,
not failures or kills. No heavy member came close to the 3240 s ceiling in force
at the time of measurement; the largest was 545.2 s.

## Where the derived figure lives

The ceiling derived from this measurement, the four constraints it was checked
against, and the reachability argument that fixes its upper bound are in
`docs/prds/offline-deep-test-lane.md` DA6. `.config/nextest.toml` carries the
value and a pointer. Do not restate the derivation here.
