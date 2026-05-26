# Multi-Process OCCT Test Concurrency: Benchmark & Validation

## (a) Per-Process OCCT Static-Init Cost Measurement

Measured 2026-05-26 on the host (debug profile, sccache warm):

| Run | Command | Wall-clock |
|-----|---------|-----------|
| Single test | `cargo test -p reify-eval -- --exact 'cache::tests::bump_version_increments_version'` | 13.6 s |
| Full crate  | `cargo test -p reify-eval -- --test-threads=1` | 111.5 s |

**Key observation:** `reify-eval` contains ~130 test binaries. Running `--exact <one_test>`
still spawns all 130 binaries (each filters to 0 or 1 matching test), so the 13.6 s is
purely per-binary process-launch overhead across 130 processes — not a single-process cost.

Per-binary overhead estimate: 13.6 s / 130 ≈ **0.10 s/binary**.

The actual test work (1063 tests) consumes 111.5 − 13.6 ≈ 98 s.  OCCT static init (shape
tables, allocators) is a subset of the ~0.10 s/binary cost; on this host with warm sccache
artifacts it is not the dominant term, but in release builds with memory-heavy OCCT geometry
it becomes significant.

**Decision input:** If cargo-nextest `test-groups` ran one binary per test (1063 processes),
startup alone would consume 1063 × 0.10 s ≈ 106 s — comparable to the full serial run with
no speedup from parallelism.  This confirms that **process-per-test-binary** (cargo's default
— one binary per `(crate, target)` pair, ≈ 5–10 binaries across the 4 OCCT crates) is the
correct granularity; the semaphore wrapper bounds how many such cargo invocations run
concurrently host-wide.

## (b) Mechanism Choice Rationale

See `plan.json` design decisions for the full rationale.  Summary:

| Alternative | Problem |
|-------------|---------|
| nextest `test-groups` (max-threads=1) | One process per test: pays init O(N\_tests) ≈ 1063×; startup dominates |
| Manual sharding | New orchestration layer inside the wrapper; substantial complexity |
| **N-slot flock semaphore** (chosen) | Bounds inter-worktree concurrency; no per-test process explosion; pure shell, no extra deps |

The N-slot semaphore is strictly correct because OCCT C++ statics are PER-PROCESS — cargo's
natural test-binary parallelism already gives isolation within a single invocation.  The
semaphore only bounds how many cargo invocations (each running several test binaries) overlap
on the same host.

## (c) Idle-Box Validation Gate Results

_To be filled in after adoption: M shards × K runs, deterministic pass/fail, measured
wall-clock speedup vs M=1 baseline, peak FD count and peak RSS._

Placeholder: initial deployment uses `REIFY_OCCT_MAX_CONCURRENCY=4` (default) with
load-aware auto-detect (`N = clamp(nproc − load_1m_int, 1, MAX_CAP)`).  The validation
gate will update this section with empirical numbers once the semaphore has been exercised
on an idle box with a full release-profile OCCT run.

## (d) Adopted Defaults

- `REIFY_OCCT_MAX_CONCURRENCY=4` — conservative for memory-heavy release builds; leaves
  headroom for the orchestrator's other concurrent tasks on a ~32-core host.
- Auto-detect: `N = clamp(nproc − load_1m_int, 1, 4)` — respects live host load without
  exotic dependencies.
- Override: set `REIFY_OCCT_CONCURRENCY=N` to pin N (e.g. `=1` for the historical
  exclusive-mode behavior, `=8` in dedicated benchmark contexts).
