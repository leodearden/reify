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

Full measurement run (the OCCT-gated crate set, all 4 crates, debug profile):

```bash
NEXTEST_EXPERIMENTAL_LIBTEST_JSON=1 cargo nextest run \
  -p reify-kernel-occt -p reify-eval -p reify-cli -p reify-config \
  --message-format libtest-json-plus > occt-gated-run.jsonl 2>occt-gated-run.stderr

# Cross-check aggregate user+sys CPU for the same invocation:
/usr/bin/time -v cargo nextest run \
  -p reify-kernel-occt -p reify-eval -p reify-cli -p reify-config \
  > occt-gated-run.time.log 2>&1
```

Per-test `exec_time` values are summed per bucket by matching each JSON
`"name"` field (`<crate>::<binary>$<test_fn>` for integration tests,
`<crate>::<binary>` binary-relative module path for `--lib` unit tests)
against the bucket-1 module list and the bucket-2/bucket-3 file lists above.

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
| Base commit | `1d2f2db021c665ce1ee02f332a2aa019bed37e1b` (task/4933) |
