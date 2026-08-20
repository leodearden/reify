# Observed test-binary concurrency after un-narrowing the global nextest pool

Task 6018 acceptance artifact (residual deliverable of task 5984 carry-over 1,
esc-5984-2).  Instrument: `scripts/sample-test-binary-concurrency.sh`
(contract guard: `tests/infra/test_test_binary_concurrency_sampler.sh`).

**Verdict: NOT CONCLUSIVE.  The acceptance clause is NOT discharged by this
run.**  Read the "What this does and does not show" section before citing any
number here.

## Configuration actually in force

Read back from a fresh `bash scripts/gen-nextest-config.sh` on the measurement
host, not assumed:

| item | value |
|---|---|
| host `nproc` | 32 |
| generated `[profile.default] test-threads` | 32 |
| generated `[test-groups] occt max-threads` | 24 |
| in-file `.config/nextest.toml` literal | `test-threads = 32` |

Before task 6018 the generated value was 16.  The occt group cap is unchanged
at 24 and is now a genuine backstop *below* the global (Test 17i pins that
ordering).

## Window 1 — the only window that observed an execution phase

```
INFO: nextest test-binary concurrency: peak=14 samples=163 nonzero_samples=12 \
      interval=1s duration=420s deps_glob=*/target/*/deps/* host_nproc=32
```

* Workload: `cargo nextest run --config-file <generated> -p reify-syntax
  -p reify-compiler -p reify-lsp -p reify-expr -p reify-core --no-fail-fast`,
  i.e. a **scoped debug-profile nextest pass**, not a full
  `scripts/verify.sh --scope all --profile both` gate.  The lane's `target/`
  was warm (83 GiB, 2824 built deps) and the branch diff touches no `.rs`
  file, so the pass went essentially straight to execution.
* The run reached test 3107/7579 during the window and was subsequently
  terminated by the harness `timeout 400` wrapper, not by a test failure.
* Sampling was host-wide (all lanes), not scoped to this lane.
* **Taken with the cwd-dependence defect live, but UNCONTAMINATED by it** — see
  "Instrument defects found after the fact" below.  Re-confirmed first-hand
  against the pre-fix script itself: from the lane root the recorded
  `deps_glob=*/target/*/deps/*` matches nothing on disk (`for x in
  */target/*/deps/*` yields the literal pattern), so the pattern stayed literal
  and the buggy script reproduces the fixed script's answer exactly on an
  identical fixture (both `peak=1`).  `peak=14 nonzero_samples=12` is a real
  observation of that window.

## Window 2 — the INCONCLUSIVE guard firing correctly

```
INFO: nextest test-binary concurrency: peak=0 samples=339 nonzero_samples=0 \
      interval=0s duration=300s deps_glob=*/target/*/deps/* host_nproc=32
```

This denser re-run (`--interval 0`) was started to attack window 1's sparse
phase coverage, but the nextest run had already been killed by its `timeout`
before the window opened.  The sampler reported `nonzero_samples=0` and
emitted its explicit INCONCLUSIVE warning rather than presenting `peak=0` as
evidence that the pool is bounded at zero.  Recorded here because it is a live
demonstration that the defect-(b) guard works: without `nonzero_samples`, this
window is textually indistinguishable from "the bound held".

**This zero is NOT the cwd-dependence defect.**  A reader who has just learned
about that defect (below) will reasonably suspect every recorded zero, so state
it plainly: this window was recorded from the same lane root as window 1, where
the pattern demonstrably stays literal, and the cause is the already-documented
one — nextest had been killed before the window opened, so there was nothing to
count.

## What this does and does not show

The acceptance criterion is `nonzero_samples > 0` **AND** `peak > 16`.
Window 1 satisfies the first and fails the second, so it is **not** a
conclusive observation, and `peak=14` must **not** be reported as "the pool
stayed under 16".

The reason is instrument coverage, not a bound:

* Only **12 of 163 samples (7.4%)** saw any confirmed test binary at all, even
  though the run was executing continuously throughout the window.
* Each pass cost ~2.6 s wall (163 samples in 420 s) because the host was loaded
  by the run being measured — well above the 0.28 s measured on an idle host.
* The tests in flight were short: the observed per-test times were ~0.2–0.8 s.

*Hypothesis (not measured):* at this churn rate most candidates returned by the
`pgrep -f` prefilter have already exited by the time `readlink /proc/<pid>/exe`
confirms them, so the prefilter→confirm gap systematically undercounts.  That
race is real and is deliberately non-fatal (assert A5), but at ~2.6 s/pass
against ~0.4 s tests it plausibly dominates.  `peak=14` is therefore best read
as a **floor**, not a ceiling — it is consistent with a true concurrency of 32
and equally consistent with one of 14.  Testing this hypothesis requires
instrument work, not more sampling: see the follow-up below.

## Instrument defects found after the fact

**Cwd-dependent match set (fixed).**  `scripts/sample-test-binary-concurrency.sh`
iterated its pattern list as `for glob in $DEPS_GLOB` (then-current line 140).
The expansion is unquoted, so it got pathname expansion as well as the intended
word-splitting — and the default `*/target/*/deps/*` is itself a live glob.
From a cwd containing a matching tree the loop variable bound to real *relative*
paths, which can never match an absolute `/proc/<pid>/exe` target, so the
sampler silently counted zero.  Measured on one identical fixture: `peak=1` from
a lane root versus `peak=0` from `/home/leo/src/warm-lanes/worktrees`.

This mattered enough to block because a cwd-induced zero is **textually
indistinguishable** from the genuine "never observed an execution phase"
INCONCLUSIVE reading that window 2 demonstrates — i.e. it corrupts precisely the
reading this instrument exists to make trustworthy, and does so silently.

Fixed by splitting the pattern list once, at parse time, with globbing disabled
(`set -f` around the split only, so a glob-dependent `REIFY_SAMPLER_PIDS_CMD`
keeps working), and iterating the resulting array.  A whitespace-only
`--deps-glob` — which passed the old non-empty check but split to zero patterns
and then counted 0 forever — is now rejected at parse time for the same
silent-wrong-measurement reason.  The cwd regression is pinned by asserts
A10a–A10d in `tests/infra/test_test_binary_concurrency_sampler.sh`.

**Neither recorded number changed.**  Both windows above were taken from the
lane root, where the pattern stays literal; the fix is a correctness repair to
the instrument, not a revision of the data, and the verdict below is unchanged
by it.

## Host-wide framing (do not confuse with the per-run bound)

`test-threads` bounds **one** `cargo nextest run`, never the host.  With
`REIFY_TEST_SEMAPHORE_CONCURRENCY` at 1 (dark-factory-orchestrator.yaml:331,
dropped 2 -> 1 in df commit 712e6230d6) plus the merge role's bypass
(`scripts/lib_test_semaphore.sh:91`), the reachable steady state is
**2 × test-threads** — ~32 before task 6018, ~64 after.  A single lane's
observed peak is therefore expected at or below 32, not 64.

## What a valid re-run needs

1. A window covering a **full** `scripts/verify.sh --scope all --profile both`
   execution phase, not a scoped 5-crate pass.
2. `nonzero_samples` at a usable fraction of `samples` — if it comes back in
   the single-digit-percent range again, the reading is about the instrument,
   not the pool, regardless of what `peak` says.
3. Ideally an instrument fix for the prefilter→confirm race (e.g. confirming
   from a single `/proc` snapshot taken in one pass rather than re-reading per
   candidate after a slow `pgrep`).
4. No constraint on **where** the sampler is launched from — it is a host-wide
   instrument and its result is cwd-independent *as of* the fix described under
   "Instrument defects found after the fact".  But any reading taken with an
   **earlier copy** of the script must have its cwd checked before it is
   trusted: if that cwd contained a tree matching the run's `deps_glob`, the
   reading is a silent zero and is not evidence about anything.

Until then the un-narrowing is justified by the config being *read back* as
`test-threads = 32` (verified) — not by an observed peak.
