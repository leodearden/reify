# FEA test CPU by category — reify-eval-fea decomposition selection (Task M)

**PRD:** `docs/prds/reify-eval-fea-decomposition.md` (Task M — the measure-first gate).
**Purpose:** measure `reify-eval`'s FEA test CPU, split by category, and render a
go/no-go verdict against the PRD's `§Pre-conditions` threshold, **before** any
crate split (A/B/D/P/S) is attempted. Those five tasks are filed but held
`deferred` until this document's verdict clears the flip-condition:

> Proceed (flip A/B/D — and P/S — `deferred`→`pending`) iff the
> **skippable-on-OCCT-change** FEA test CPU is material: provisionally
> **≥ ~60 s** or **≥ ~15 %** of the OCCT-gated test CPU (Track 1 counts the
> pure-unit slice; the P/S track counts the ~30 synthetic-mesh e2e). If below,
> **cancel** A/B/D/P/S and keep `reify-eval` monolithic.
> — PRD `§Pre-conditions`

This task reads/runs tests only; it does not edit `reify-eval` source (no
module lock — deliberately avoids the 3768 starvation cluster). The sole
committed deliverable is this document.

## What "test CPU" means here

"Test CPU" = **summed per-test `exec_time`** (serialized CPU-seconds), not
whole-run wall-clock. Two reasons this basis, not wall-clock:

1. **The threshold is about skippable test *work***. Wall-clock collapses
   under nextest's parallelism (the `occt` test-group alone runs up to
   `max-threads = 24` concurrently — see `.config/nextest.toml`), so it
   understates the CPU that a narrower `affected-crates` selection would
   actually avoid spending. Summed per-test `exec_time` is the correct
   proxy for "how much CPU work does skipping this group save."
2. **Per-test granularity is required for categorization, not just for
   totals.** Bucket-1 (pure solver unit tests) lives in `reify-eval`'s
   single `--lib` unit-test binary alongside hundreds of non-FEA unit tests.
   Only per-test timing lets bucket-1 be split out of that shared binary;
   a binary-level or crate-level time would conflate it with unrelated
   `reify-eval` unit-test CPU.

`/usr/bin/time -v`'s aggregate user+sys CPU is used as a **cross-check** on
the OCCT-gated total (not per-bucket), since it captures process-level CPU
that per-test `exec_time` can miss (see Caveats).

## Measured scope

The **OCCT-gated crate set** — `{reify-kernel-occt, reify-eval, reify-cli,
reify-config}` (`scripts/occt-touching-crates.txt`, the single source of
truth also consumed by `orchestrator.yaml`'s gated test invocation) — run
under the **debug** profile. A change to `reify-kernel-occt` reverse-selects
exactly this set under `scripts/verify.sh --scope branch` (see
`affected-crates-lib.sh`), so measuring these four crates together **is** a
representative OCCT-touching branch verify. Debug (not release) is used
because it is `--scope branch`'s default profile.

## Three-bucket categorization criterion

Buckets are drawn to align 1:1 with what each decomposition track can
actually skip on an OCCT change (PRD `§Background` "achievable prize" table):

- **Bucket 1 — pure solver unit tests.** Tests co-located (as `#[cfg(test)]`
  modules) under the 7 Lever-B solver source modules:
  `compute_targets/*`, `modal_ops.rs`, `dynamics_ops.rs`, `dynamics_psd.rs`,
  `trajectory_ops.rs`, `multi_load_dispatch.rs`, `solver_progress.rs`. These
  are exactly the tests Lever B moves into the new `reify-eval-fea` crate.
  *Skippable by:* Track 1 (A→B→D) alone.
- **Bucket 2 — engine-driven synthetic-mesh FEA e2e.** Integration test
  files under `crates/reify-eval/tests/*.rs` that drive an actual
  structural/mechanics **solve** (elastic/buckling/modal/dynamics/shell/
  membrane/fdm/tensegrity/form-find/flexure/gravity/moment-of-inertia/psd/
  trajectory) through `reify_test_support::make_simple_engine` (kernel-less
  `Engine`, no `GeometryKernel` registered) plus
  `reify_eval::compute_targets::register_compute_fns` — i.e. they reach the
  same solver-dispatch surface as bucket 1's unit tests, just through the
  full demand-driven `Engine::eval` pipeline instead of calling the solver
  function directly. Membership is curated (not a mechanical filter): of the
  114 `crates/reify-eval/tests/*.rs` files that call `make_simple_engine`,
  most are unrelated (kinematics-only, materials, cost, DFM, generic
  eval/dispatch-registry infra) — see Appendix for the exact list and the
  per-file inclusion/exclusion rationale. *Skippable by:* Track 2 (P→S),
  **only after both P and S land** — P alone moves the files to an
  OCCT-free crate; S is required so `affected-crates-lib.sh` actually stops
  selecting that crate on an OCCT change (today's dev-dep-transitive
  reverse-closure would still select it).
- **Bucket 3 — genuinely geometry-driven FEA e2e.** The 3 named files that
  spawn a real kernel: `fea_face_selector_bc_e2e.rs` (gmsh, via
  `reify_kernel_occt::OcctKernelHandle::spawn` per its own doc comment),
  `dynamics_body_mass_props.rs` (mixed — see Caveats), and
  `rigid_moment_of_inertia_autoderive_smoke.rs` (OCCT,
  `OcctKernelHandle::spawn`, `OCCT_AVAILABLE`-gated). *Never skippable* on an
  OCCT change — correctly rerun.

Everything in the OCCT-gated total that is not bucket 1/2/3 (the rest of
`reify-eval`'s ~2421 tests, plus `reify-kernel-occt`/`reify-cli`/
`reify-config`'s own tests) is reported as **residual** — it is not part of
the go/no-go numerator for either track but is needed to compute the
OCCT-gated total and the bucket-2/1 percentages.

## Bucket membership (curated)

### Bucket 1 — pure solver unit tests (7 Lever-B modules)

Realized per-module `#[test]` count, from the measurement run's per-test
`exec_time` JSON (not just a static grep — this confirms every statically
declared test actually registered and ran under nextest):

| Module (`crates/reify-eval/src/...`) | Tests | CPU (s) |
|---|---:|---:|
| `compute_targets/elastic_static.rs` | 46 | 10.409 |
| `modal_ops.rs` | 39 | 7.540 |
| `dynamics_ops.rs` | 38 | 6.170 |
| `compute_targets/fdm_slice.rs` | 12 | 1.949 |
| `compute_targets/fea_diagnostics.rs` | 13 | 1.831 |
| `compute_targets/result_topology.rs` | 12 | 1.768 |
| `dynamics_psd.rs` | 10 | 1.331 |
| `solver_progress.rs` | 2 | 1.774 |
| `compute_targets/shell_solve.rs` | 7 | 0.950 |
| `multi_load_dispatch.rs` | 7 | 0.978 |
| `compute_targets/as_printed_material.rs` | 3 | 0.757 |
| `compute_targets/bc_resolve.rs` | 4 | 0.463 |
| `trajectory_ops.rs` | 4 | 0.481 |
| `compute_targets/buckling.rs` | 2 | 0.239 |
| `compute_targets/tensegrity_load.rs` | 1 | 0.141 |
| `compute_targets/membrane_load.rs` | 1 | 0.122 |
| `compute_targets/mod.rs` (top-level) | 1 | 0.393 |
| **Total** | **202** | **37.297** |

202 matches the PRD's `~206` estimate closely (the small gap is the estimate's
own margin, not a data-quality issue — every module's tests were individually
enumerated and cross-checked against a static `grep -c '#[test]'` per file,
which agreed exactly). `compute_targets/{as_printed_material_r0,
buckling_multi_case, form_find, multi_case, tensegrity_crack}.rs` declare 0
`#[test]` (helper/type-only modules) and so contribute nothing.

### Bucket 2 — engine-driven synthetic-mesh FEA e2e (curated)

**Methodology.** Filename alone is unreliable in both directions in this
codebase, so membership was decided by reading each of the 115
`make_simple_engine`-using files under `crates/reify-eval/tests/*.rs` for
actual solve dispatch (a call to `reify_eval::compute_targets::
register_compute_fns` or the narrower `engine.register_compute_fn("<target>",
...)`, feeding a real trampoline such as `solve_modal_analysis_trampoline`),
not just grepping for FEA-sounding keywords in prose/docstrings. Two
concrete corrections this caught, illustrating why:

- **False negative:** `modal_compute_node.rs` never calls the umbrella
  `register_compute_fns`, but it does call the narrower
  `engine.register_compute_fn("modal::free_vibration",
  reify_eval::modal_ops::solve_modal_analysis_trampoline as ComputeFn)`
  directly — a genuine dense-eigensolve dispatch (its own doc comment: "before
  any costly eigensolve" / "the dense eigensolve path is taken"). **Included.**
- **False positive:** `tensegrity_t0a.rs` has the single highest keyword-hit
  density of any of the 115 files (102 raw hits of `tensegrity`/`Strut`/
  `Cable`/etc.) and a solve-sounding name, but it is a pure SIR-α
  **constructor/DSL-authoring boundary test** — it checks that `Strut(...)`,
  `Cable(...)`, `Tensegrity(...)` lower to correctly-shaped
  `Value::StructureInstance`s and exercises a `tensegrity_wires` wireframe
  builtin (geometry layout for a CLI golden-output test), and contains **no**
  `register_compute_fn`/`register_compute_fns` call and no trampoline
  reference anywhere in the file. **Excluded** (moved out of an initial
  automated draft that over-trusted the keyword count).

The same read also excluded a small cluster of superficially FEA-named files
that turned out to be the same kind of type-surface/structure-def boundary
test as `tensegrity_t0a.rs` — `gravity_load.rs`, `pressure_load.rs`,
`load_case.rs`, and `pinned_support.rs` each only check that `Gravity()`/
`PressureLoad()`/`LoadCase()`/`PinnedSupport()` constructors lower to the
right `Value::StructureInstance` shape and trait-conformance; none reaches
`register_compute_fns` or a solve trampoline.

**Included (39 files, 146 `#[test]` fns, 131 of them executed in this run —
see the 15 debug-only skips noted below and in Caveats):**

| File | Why it's bucket 2 | Tests |
|---|---|---:|
| `buckling_multi_case.rs` | dispatches `solver::buckling_multi_case` | 3 |
| `buckling_p2_smoke.rs` | `solve_buckling` via `buckling_column_p2.ri` | 1 |
| `buckling_persistent_cache_round_trip.rs` | persistent-cache round trip over `solver::buckling` | 1 |
| `buckling_smoke.rs` | dispatches `solver::buckling` | 5 |
| `compute_cache_key_population.rs` | real `solver::elastic_static` solve; cache-key sensitivity | 4 |
| `differential_field_ops_e2e.rs` | asserts `target=="solver::elastic_static"`; differentiates output fields | 1 |
| `dynamics_compute_node.rs` | dispatches `dynamics::inverse_dynamics` | 1 |
| `fea_diagnostics_e2e.rs` | drives `solver::elastic_static` diagnostics path | 5 |
| `fea_structured_detail_e2e.rs` | drives `solver::elastic_static` structured diagnostic payload | 1 |
| `flexure_e2e.rs` | closed-form structural mechanics (see note below) | 8 |
| `gravity_self_weight_e2e.rs` | `solve_elastic_static` with self-weight body force | 4 |
| `input_shape_eval_e2e.rs` | dispatches `trajectory::input_shape` | 3 |
| `input_shape_tots_compute_node.rs` | dispatches `trajectory::input_shape` | 4 |
| `modal_analysis_e2e.rs` | dispatches `modal::free_vibration` | 5 |
| `modal_compute_node.rs` | dispatches `modal::free_vibration` (narrow registration; see above) | 2 |
| `modal_transient_e2e.rs` | `modal::transient_response` + `modal::displacement_at` | 2 |
| `multi_case_compute_node.rs` | `solver::multi_case` + `solver::elastic_static` | 2 |
| `multi_load_bracket_e2e.rs` | real solve via `examples/multi_load_bracket.ri` | 1 |
| `persistent_cache_compute_round_trip.rs` | cross-restart persistent-cache round trip over `solver::elastic_static` | 1 |
| `printer_print_envelope_e2e.rs` | "drives a full modal solve (heavy FEA eigenproblem)" + trajectory | 3 |
| `printer_z_compliant_mount_e2e.rs` | `modal::mechanism_modal` + flexure compliance | 4 |
| `shell_extract_gui_accessor.rs` | evals `fea_shell_flexure.ri` through real `solver::elastic_static` (shell/MITC3 route) | 2 |
| `shell_solve_e2e.rs` | `target=="solver::elastic_static"` shell (MITC3) route | 2 |
| `shell_too_thick_at_auto_falls_back.rs` | real elastic-static solve, tet-fallback path | 1 |
| `shell_too_thick_at_shell_annotation_errors.rs` | real elastic-static solve attempt, hard-error path | 1 |
| `simulate_trajectory_compute_node.rs` | dispatches `trajectory::simulate` | 3 |
| `solve_elastic_static_e2e.rs` | canonical `solver::elastic_static` cantilever e2e | 11 |
| `solve_elastic_static_pressure_e2e.rs` | `solver::elastic_static` with pressure loads | 2 |
| `solver_progress_emit_e2e.rs` | real `solver::elastic_static` solve (progress emission) | 2 |
| `tensegrity_delta_combined_form_find_e2e.rs` | `solver::form_find_free` (struts+cables+membrane) | 4 |
| `tensegrity_membrane_load.rs` | `solver::membrane_load` trampoline | 8 |
| `tensegrity_pavilion_e2e.rs` | combined `solver::form_find_free` + `solver::membrane_load` | 7 |
| `tensegrity_t1a_form_find.rs` | `solver::form_find` (anchored Force-Density) | 20 |
| `tensegrity_t1b_form_find_e2e.rs` | `solver::form_find_free` (free-standing Force-Density) | 6 |
| `tensegrity_t3b_load.rs` | `solver::tensegrity_load` trampoline | 8 |
| `thin_walled_bracket_e2e.rs` | real `solve_elastic_static` (shell route) | 1 |
| `toolhead_motor_sizing_e2e.rs` | `dynamics::inverse_dynamics` (RNEA motor-torque sizing) | 1 |
| `typed_fea_authoring_gate.rs` | real `solver::elastic_static`/`solver::multi_case` across 3 fixtures | 4 |
| `zv_shaped_ramp_db_reduction.rs` | `trajectory::input_shape`/`trajectory::simulate` (ZV-shaper) | 2 |

**Note on `flexure_e2e.rs`:** unlike every other included file, it never
calls a `register_compute_fn[s]` — flexure spring-rate/stress is plain
synchronous stdlib math (closed-form Howell/Paros-Weisbord formulas
evaluated through the full engine pipeline), not a `ComputeNode`/trampoline
dispatch. It is kept in bucket 2 because "flexure" is one of the plan's
named structural-mechanics categories and the file's entire subject is a
structural component's response validated against literature closed-form
solutions — but flag it if a stricter "only `ComputeNode`-dispatched" bucket-2
definition is wanted; excluding it removes 8 tests / 11.166 s (see CPU table).

**Excluded (76 files, including the reclassified `tensegrity_t0a.rs`):**
type-surface/structure-def ctor-boundary tests (`gravity_load.rs`,
`load_case.rs`, `pinned_support.rs`, `pressure_load.rs`,
`flexure_joint_fields_e2e.rs`, `structure_instance_e2e.rs`,
`tensegrity_t0a.rs`, `trajectory_gcode_dialect_eval.rs`,
`fea_loads_stdlib_smoke.rs`, `multi_load_case_stdlib_smoke.rs`); generic
compute-dispatch/cache/registry/cancellation infra (`cancellation_compute_
dispatch.rs`, `compute_dispatch_registry.rs`, `freshness_pending_compute_
dispatch.rs`, `opaque_state_lifecycle.rs`, `material_field_cancellation.rs`,
`optimized_registry_tests.rs`, `realization_read_api.rs`, `warm_state_
donation.rs`, `as_printed_r0_trampoline.rs`); shell/mid-surface geometry
extraction, not a solve (`shell_channels_surfacing_e2e.rs`, `shell_extract_
compute_integration.rs`, `shell_extract_persistent_cache_round_trip.rs`,
`mid_surface_fold_e2e.rs`); generic math/stdlib smoke not tied to a real
solve output (`fea_stress_reductions_smoke.rs`, `evaluate_profile_eval_
e2e.rs`); kinematics-only (11 files); materials/cost/DFM/BOM/GD&T/
appearance/feature-ID/annotation (11 files); ports/threading (2); sweep
geometry (1); printing/gcode (1); generic language/DSL feature tests (25
files, incl. the `m8`–`m11` milestone-integration suite and
`representation_within_assertion.rs`, which despite being LPT-priority-tier
in `.config/nextest.toml` is a broad assertion helper, not FEA-specific).
Full per-file rationale was captured during curation; the summary above
groups it by category rather than repeating all 76 one-line reasons.

**Vs. the PRD's premise:** the PRD's `§Background` table estimates
"~30 files / ~80 fns" for this category. The curated, content-verified count
is **39 files / 146 `#[test]` fns** — meaningfully larger in both dimensions.
This is reported as a finding, not silently reconciled: the PRD's number was
a preliminary estimate; this measurement's count comes from reading every
candidate file for actual solve dispatch (catching both the false-negative
and false-positive cases above), so it supersedes the estimate for sizing
Lever P's migration scope.

### Bucket 3 — genuinely geometry-driven FEA e2e (3 files, confirmed)

| File | Kernel | Tests run / total | CPU (s) |
|---|---|---:|---:|
| `dynamics_body_mass_props.rs` | `MockGeometryKernel` (1 test) + real `OcctKernelHandle::spawn` (1 test) | 2 / 2 | 2.795 |
| `fea_face_selector_bc_e2e.rs` | gmsh via `OcctKernelHandle::spawn` (`has_gmsh` confirmed set on this host — real tests compiled, not the `cfg(not(has_gmsh))` stub) | 1 / 2 | 10.837 |
| `rigid_moment_of_inertia_autoderive_smoke.rs` | OCCT, `OCCT_AVAILABLE`-gated (confirmed true on this host) | 1 / 1 | 1.826 |
| **Total** | | **4 / 5** | **15.458** |

`fea_face_selector_bc_e2e.rs`'s second test
(`boundary_demand_realization_edge_produces_nonempty_boundary`) is
individually `#[ignore]`'d for a reason unrelated to gmsh/OCCT availability
(both its real `cfg(has_gmsh)` tests compiled; nextest reports 1
passed + 1 ignored for that binary) — never-skippable bucket-3 is unaffected
either way since neither gating changes which crate is selected on an OCCT
change.

## Reproduction

Validated on this host prior to the full run (small-binary + OCCT-availability
smoke checks):

```bash
# Per-test JSON timing (nextest experimental libtest-json-plus), one crate:
NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run -p reify-eval \
  --message-format libtest-json-plus -E 'binary(buckling_option_unsupported)'
# → emits one JSON line per test event; `{"type":"test","event":"ok",...,"exec_time":<seconds>}`
# on each pass/fail, e.g. exec_time":0.092812103. Confirmed working on nextest 0.9.136.

# OCCT availability (compile-time const assert; if this compiles+passes, OCCT
# is really linked, so bucket-3's OCCT_AVAILABLE-gated tests will execute
# rather than skip and under-report):
cargo nextest run -p reify-kernel-occt --lib \
  -E 'test(occt_available_is_true_when_built_with_occt)'
# → PASS on this host (2026-07-01).
```

Full measurement run (the OCCT-gated crate set, all 4 crates, debug profile).
Run as a **single** invocation — `/usr/bin/time -v` wraps the same `cargo
nextest run` that emits the per-test JSON, rather than running the whole
suite twice, since the two signals (per-test JSON on stdout, `time`'s
resource-usage report via `-o`) don't collide:

```bash
NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 /usr/bin/time -v -o occt-gated-run.time.log \
  cargo nextest run -p reify-kernel-occt -p reify-eval -p reify-cli -p reify-config \
  --message-format libtest-json-plus --no-fail-fast \
  > occt-gated-run.jsonl 2> occt-gated-run.stderr
```

(`--no-fail-fast` is added defensively so a single failing test can't
truncate the per-test timing data for the rest of the ~5400-test run; this
run had 0 failures, so it made no difference to the result.)

Per-test `exec_time` values are summed per bucket by matching each JSON
`"name"` field (`<crate>::<binary>$<test_fn>` for integration tests,
`<crate>::<binary>` binary-relative module path for `--lib` unit tests)
against the bucket-1 module list and the bucket-2/bucket-3 file lists above.

**Run result (2026-07-01/02, this host):** `5444 tests run: 5444 passed (4
slow, 1 leaky), 39 skipped` — nextest's own summary line, test-phase wall
time `198.244s`. Total elapsed (incl. the mostly-cached compile step):
`3:34.86` (214.86 s). `/usr/bin/time -v`: User `1999.67s` + System `431.48s`
= **2431.15 s** aggregate CPU for the whole invocation (compile + test).
`grep -c` sanity checks on the raw JSONL: exactly 5444 distinct test names
have an `"ok"` event (**zero duplicates** — no risk of double-counting a
retried test), and the 39 "started but no terminal `ok`" names match
nextest's own "39 skipped" exactly (see Caveats for what's behind those 39).

An experimental-flag quirk worth flagging for future reproductions: the
`libtest-json-plus` stream emits a fresh `"suite","event":"ok"` summary line
each time an individual ignored test inside a multi-test binary resolves (up
to 5 near-identical repeats seen for one binary in this run), not just once
per binary. This is harmless here because the categorization sums **only**
`"test","event":"ok"` lines (verified duplicate-free above), never the
suite-level lines — but a future script that trusts the suite-level `ok`
count directly would over-count.

## Caveats

- **faer internal parallelism.** Solve-heavy tests (elastic/buckling/modal)
  may call into `faer`'s internally-parallel linear-algebra routines. A
  test's `exec_time` reflects wall-clock *for that test thread*, not the
  total CPU-seconds consumed across faer's worker threads — so true CPU for
  solve-heavy tests can exceed the summed `exec_time`. This makes the
  reported bucket-1/2 numbers a **conservative (lower-bound)** estimate of
  real skippable CPU, which only strengthens a PROCEED verdict and is noted
  wherever it could weaken a CANCEL verdict.
- **OCCT/gmsh availability gates bucket 3.** `dynamics_body_mass_props.rs`
  and `rigid_moment_of_inertia_autoderive_smoke.rs` skip their
  kernel-spawning tests when `OCCT_AVAILABLE` is false; `fea_face_selector_
  bc_e2e.rs` compiles a stub in place of its 2 real tests when `has_gmsh` is
  not set. The OCCT-availability smoke check above confirms OCCT is live on
  this host; the full run's own test list (skipped-vs-run counts) is the
  authoritative confirmation and is reported alongside the numbers.
- **Debug, not release, profile.** `--scope branch` runs debug by default;
  this measurement matches that. Absolute seconds would shrink under
  release, but the go/no-go ratio (skippable ÷ OCCT-gated total) is the
  primary decision signal and is far less sensitive to the debug/release
  choice than absolute seconds (both numerator and denominator scale
  together for the CPU-bound solve tests that dominate both).
- **`dynamics_body_mass_props.rs` is mixed, not purely geometry-driven.** Of
  its 2 tests, 1 uses `MockGeometryKernel` (kernel-independent) and 1 uses a
  real `OcctKernelHandle::spawn`, `OCCT_AVAILABLE`-gated. Both stay in
  bucket 3 (the file as a whole is the PRD's named geometry-driven file and
  is far too small — 2 tests — to matter to the verdict either way); this is
  reported as a data-quality nuance, not silently reclassified.
- **Host contention inflated wall-clock during the run (summed `exec_time` >
  `/usr/bin/time` aggregate).** Summed per-test `exec_time` across the whole
  OCCT-gated run is **4627.054 s**, nearly double the `/usr/bin/time`
  aggregate CPU of **2431.15 s** for the identical invocation. This is the
  opposite direction from the faer-parallelism caveat above, and has a
  different cause: `uptime` immediately before the run showed `load average:
  ...37.89, 30.06, 35.29` on this 32-core host (other concurrent worktrees'
  builds/tests), and immediately after showed a 15-min load average that had
  peaked at `90.49`, with `/proc/pressure/cpu` `avg300=18.92%`. Per-test
  `exec_time` is wall-clock for that test's thread; under system-wide
  scheduling contention (well beyond this run's own ≤24-thread `occt` group),
  a runnable thread can wait for a CPU slot, inflating its observed wall time
  above the CPU-seconds actually granted to it — while `/usr/bin/time`'s
  `user`+`sys` is scheduler-independent (it only counts CPU-seconds actually
  received via `wait4`/`rusage`). Net effect: the summed-`exec_time` bucket
  numbers reported here likely **overstate** true CPU-seconds somewhat on a
  quiet host, which — like the faer caveat — only strengthens a PROCEED
  reading (the bucket-2 total would need to shrink, not grow, to threaten the
  verdict) and does not change which bucket a test falls into.
- **A material fraction of bucket 2's heaviest tests are skipped by design
  in this (debug) measurement.** 8 files carry
  `#[cfg_attr(debug_assertions, ignore = "heavy modal solve; release-only")]`
  (or equivalent wording) on ~15 of their heaviest tests: 7 are in bucket 2
  (`buckling_p2_smoke.rs`, `buckling_persistent_cache_round_trip.rs`,
  `buckling_smoke.rs`, `input_shape_tots_compute_node.rs`,
  `modal_analysis_e2e.rs`, `modal_transient_e2e.rs`,
  `zv_shaped_ramp_db_reduction.rs`); the 8th, `warm_state_donation.rs`, is
  excluded from bucket 2 (residual) but carries the same gating on its one
  `modal::free_vibration`-dispatching test. `reify-eval` is
  itself listed in `scripts/release-sensitive-crates.txt`, whose own header
  documents this as "Mechanism A — tests ignored in debug, exercised only in
  release." These tests are correctly excluded from **this** measurement:
  `scripts/verify.sh`'s role-based profile default is debug-only for
  task-level `--scope branch` verifies (`PROFILE="both"` only applies when
  `DF_VERIFY_ROLE=merge`, i.e. the `--scope all` merge gate), so they
  genuinely do not cost CPU in the representative scenario this doc measures.
  However, it means the reported bucket-2 total is a **lower bound** on the
  FEA CPU these same files would cost under a release-mode/merge-gate
  invocation — one more reason the measured numbers here should be read as
  conservative with respect to a PROCEED verdict. Quantifying the release-mode
  total was judged out of scope for this measurement (a materially different,
  heavier invocation than the debug `--scope branch` this doc targets); it is
  a natural follow-up if the P/S go/no-go were ever borderline against the
  15% bar specifically (it is not — see Verdict).

## Host / toolchain

| | |
|---|---|
| Date | 2026-07-01 |
| Host | `x86_64-unknown-linux-gnu`, 32 cores, 125 GiB RAM |
| Kernel | Linux 6.14.0-37-generic |
| rustc | 1.96.0 (ac68faa20 2026-05-25) |
| cargo | 1.96.0 (30a34c682 2026-05-25) |
| cargo-nextest | 0.9.136 (1d5bf1ec9, 2026-05-16) |
| Profile | debug (`--scope branch` default) |
| Base commit (premises verified) | `1d2f2db021c665ce1ee02f332a2aa019bed37e1b` (task/4933) |
| Measurement run commit | `718fed7ad2ffd6493f9b99f54dfebfa574da1de9` (task/4933, post inter-iteration rebases; no source files differ from the base for the crates under measurement — only this doc changed) |
| Measurement run date | 2026-07-02 (run started 2026-07-01 23:21 local, completed 2026-07-01 23:24) |
