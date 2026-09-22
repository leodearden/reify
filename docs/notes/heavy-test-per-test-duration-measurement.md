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

## Where the derived figure lives

The ceiling derived from this measurement, the four constraints it was checked
against, and the reachability argument that fixes its upper bound are in
`docs/prds/offline-deep-test-lane.md` DA6. `.config/nextest.toml` carries the
value and a pointer. Do not restate the derivation here.
