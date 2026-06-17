# Phase 0 — instrumented merge-gate baseline (idle box)

Measurement spike for `docs/prds/warmer-builds-merge-verify.md` Phase 0. Run 2026-06-09 on an
**idle box** (orchestrator stopped + held down; only the desktop + a spawned prebuilt-fix `/do`
agent's playwright/e2e ran in the background — modest, ~3–5 cores). Command, per attempt:

```
DF_VERIFY_ROLE=merge  RUSTC_WRAPPER=sccache  CARGO_INCREMENTAL=0  (CARGO_MAKEFLAGS unset → -j32)
/usr/bin/time -v ./scripts/verify.sh all --profile both --scope all
```

run in a throwaway detached worktree on data_lv at `main f8f860a21b`. Two back-to-back runs:
- **COLD** — empty `target/` (today's every-attempt state).
- **WARM** — Phase-1 reset-in-place (`git reset --hard <sha> && git clean -xfd -e target`, same
  commit, no source edit, `target/` retained = 111 GB). Both passed (`rc=0`).

Raw logs: `.orchestrator-scratch/phase0/{cold,warm}.log` (per-line elapsed-seconds prefixed),
`sccache-{pre,after-cold,after-warm}.txt`, `loadsamples.tsv`.

## Headline

| | COLD (empty target/) | WARM (Phase-1 reset-in-place) | Δ (warmth removes) |
|---|---:|---:|---:|
| **Total wall** | **1751 s ≈ 29.2 min** | **656 s ≈ 10.9 min** | **1095 s ≈ 18.3 min (−63%)** |
| Compile+link buckets | ~1015 s | ~9 s (all fingerprint hits) | ~1006 s |
| Test-exec + GUI floor | ~733 s | ~643 s | ~0 (floor; warmth can't touch) |
| sccache Rust compilations | +588 (≈63% hit) | **+0** (zero recompile) | — |

**Two load-bearing findings:**

1. **A cold full merge verify is ~29 min on an idle box — not the ~90-min median.** The observed
   80–148 min merge times are therefore **~3–5× inflated by contention** (24 task lanes starving
   the serial `_MERGE_AHEAD_BOUND=1` lane), not by cold-build cost. The livelock is
   **contention-dominated** → Phase 1's CPU-second reduction *and* #4448 fail-fast are the real
   livelock-enders, more than raw wall-time on an idle box.
2. **Phase 1 (warm worktree) is validated as the keystone:** it removes the **entire** compile+link
   bucket (cold 1015 s → warm ~9 s), taking an unchanged-tree verify from **29 → 11 min on idle**
   (−63%), and proportionally more under contention. The warm run recompiled **nothing**
   (sccache compilations +0) — empirical proof that reset-in-place + retained `target/` yields a
   pure cargo-fingerprint pass, exactly as the PRD §10 invariants assume.

## Per-phase breakdown (elapsed-second boundaries)

| Phase (merge plan order) | COLD | WARM | Bucket |
|---|---:|---:|---|
| preflight (manifold + tree-sitter) | ~1 s | ~1 s | fixed |
| `clippy --workspace --all-targets` | **156 s** | 1 s | compile (A/B) |
| `cargo check -p reify-gui --features gui --tests` | **114 s** | 6 s | compile (A/B) |
| debug OCCT-gated **compile** (occt/eval/cli/config) | **182 s** | 0 s | compile (A/B) |
| debug OCCT-gated **exec** (`--test-threads=1`, serial) | **202 s** | **119 s** | floor (C) |
| debug ungated nextest **compile** | **170 s** | 1 s | compile (A/B) |
| debug ungated nextest **exec** (11 424 tests) | 123 s | **144 s** | floor (C) |
| release OCCT-gated **compile** (`reify-eval --release`) | **213 s** | 0 s | compile (A/B) |
| release OCCT-gated **exec** (`--test-threads=1`, serial) | **233 s** | **219 s** | floor (C) |
| release ungated nextest **compile** (7 crates, post-#4390) | **180 s** | 0 s | compile (A/B) |
| release ungated nextest **exec** (4 789 tests) | 131 s | 117 s | floor (C) |
| GUI (`npm ci`+`tsc`+vitest) + sidecar + tree-sitter | ~44 s | ~44 s | floor (C) |
| **Total** | **1751 s** | **656 s** | |

## What this says about each phase

- **Phase 1 (warm) — keystone, confirmed.** Eliminates the ~17-min compile+link bucket (all six
  compile phases → ~0). 29→11 min idle; the dominant lever.
- **The RELEASE pass is ~43% of the cold verify (~12.6 min)** even after #4390's crate-scoping:
  ~6.6 min release **compile** (a disjoint second compile, warmed away by Phase 1) + ~6.0 min
  release **exec** (largely the serial `reify-eval --release --test-threads=1` OCCT pass = 219 s
  even warm). → Phase 1 warms the compile half; the release-OCCT serial exec is Phase 4 territory.
- **Phase 4 (OCCT → nextest) target quantified:** serial `--test-threads=1` OCCT exec =
  **119 s (debug) + 219 s (release) ≈ 5.6 min** that *survives warmth*. Confirms the PRD-D5 premise:
  even though the gated wrapper acquires a 32-slot semaphore (3767), within the single serial merge
  invocation OCCT runs single-process serial — so 3767 did **not** help the gate here. Folding into
  the nextest pool (occt group `max-threads`>1) attacks this floor.
- **Phase 2 (linker):** the ~17-min compile bucket embeds the ~745 bfd links; not separable from
  this log, but it is the largest single bucket and is also Phase 1's worst-case (a low-level change
  that relinks broadly). α benchmarks rust-lld vs mold directly.
- **Phase 3 (debuginfo):** `target/` reached **111 GB** for one warm worktree — the disk-pressure
  dynamic Phase 3 eases is real.

## Test-exec floor composition — a few heavy tests, not a big bucket

The ~11-min warm floor is **tail-latency-bound, not CPU-bound**. Each nextest pass saturates 32
cores for its first ~30–60 s clearing ~11k fast tests, then spends its final 60–120 s on a handful
of long tests with most cores idle. The long poles are all **`reify-solver-elastic`**:

- **Debug nextest** (`Summary [120.8s]`, 11 424 tests): the **4** `SLOW [>60 s]` are *all*
  `determinism::*` — parallel-tolerance / bit-stability **thread-count sweeps** (run the solver at
  1/2/4/… threads and compare).
- **Release nextest** (`Summary [130.7s]`, 4 789 tests): **10** slow, including one **`SLOW [>120 s]`**
  (`default_parallel_tolerance_equivalent_across_thread_counts`) that alone nearly spans the pass —
  plus P2 analytical validation (cantilever, thick-walled cylinder), euler-column buckling, modal
  benchmarks.
- **Serial OCCT pass** dominated by **2 binaries** — ~113 s (5 tests) + ~95 s (14 tests) of heavy
  `reify-eval` FEA numeric tests.

These run in **both** debug and release passes (paid ~twice). **Lever (orthogonal to the build
phases):** tier the `determinism *_across_thread_counts` sweeps + P2 validation/buckling/modal
benchmarks off the merge gate (nightly / dedicated suite), reduce the swept thread counts, and/or
scope them to one profile. Composes with Phase 4 (which parallelizes the serial OCCT bucket but
leaves the single ~113 s test as the new floor).

## Why contended ≈ 3× idle: admission, not priority

Finding #1 (idle cold 29 min vs observed contended 80–148 min) is **contention**, and it is *not*
a CPU-priority problem. verify.sh already differentiates — **task** verifies run at
`nice -n 15 ionice -c2 -n7`, **merge** verifies at `nice -n 5` (no ionice) — a substantial gap. But:

- The merge verify draws rustc tokens from the **same 32-token cargo jobserver**
  (`/tmp/reify-jobserver`; `verify.py:1632` applies `verify_env` to both roles) as up to
  **24 task lanes** (`max_concurrent_tasks: 24`) — ~25 consumers, one pool. **Token hand-off is
  priority-blind:** a merge `rustc` blocked waiting for a token is *not runnable*, so its `nice -5`
  never gets to matter. `nice`/`ionice` govern *scheduling among runnable threads*; the jobserver
  governs *admission* — and the merge starves at admission. (The OCCT host-semaphore and RAM/swap
  pressure are likewise priority-blind.)
- The **PSI start gate is not a factor**: the merge role **bypasses** it (`psi-gate bypass
  (role=merge)`); `psi_gate()` only throttles *task-lane* test-phase dispatch on `cpu some avg10`.

Phase 1 helps this *indirectly* — warm builds cut the merge verify's token demand to ~zero (which
is why warm hit 11 min even while sharing the pool). The **direct** lever — a reserved merge-lane
jobserver budget and/or throttling task concurrency during the serial merge verify — is deferred to
a dedicated design session (tracked separately).

## Corrections to the design doc (from measured data)

- **Debug ungated nextest = 11 424 tests**, release ungated nextest = **4 789** tests. The design's
  "merge-gate ungated = 4789" (§7) is the **release** pass count only; the **debug** pass runs 11 424.
- The cold-vs-contended gap (29 vs ~90 min) makes **contention**, not cold-build, the headline cost —
  a sharper framing than the design's §2 "raw sum ≈ 50–85 min" (that sum overstated the idle cold
  build; the real idle cold is ~29 min, with the rest being contention).

## Method notes / caveats

- Near-idle, not perfectly idle: a spawned prebuilt-fix `/do` agent ran playwright/e2e + a long-lived
  python3 (~3–5 cores) throughout, and a ~2-min accidental orchestrator start contended the cold
  tail (~15:10–15:12 BST). Both are modest vs the 32-core box; treat the numbers as a tight
  upper-bound-on-idle, lower-bound-on-contended.
- `CARGO_MAKEFLAGS` was left unset (the shared jobserver FIFO bypassed) → cargo used `-j32`,
  equivalent to a *solo* merge verify getting the whole box. Faithful to the uncontended case.
- sccache untouched (deps stay ~60% warm, as the real gate sees them); "cold" = cold `target/`, not
  cold sccache.
