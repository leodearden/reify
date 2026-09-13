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
| in-file `.config/nextest.toml` template value | `test-threads = "num-cpus"` (was the bare literal `32` until task 6374; this row records the TEMPLATE value, not a resolved one) |

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
* **Taken with the prefilter→confirm race LIVE, and CONTAMINATED by it.**  That
  race — hypothesised below when this window was written, measured since, and
  fixed under task 6375 — undercounted by 35–65% on the same host within the
  same second.  `peak=14` is therefore a **floor**, not a bound that held, and
  this window is **not** comparable like-for-like with window 3.

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

## Window 3 — a full-gate execution phase, with the race fixed (task 6375)

```
INFO: nextest test-binary concurrency: peak=51 samples=1034 nonzero_samples=827 \
      interval=1s duration=1200s deps_glob=*/target/*/deps/* host_nproc=32
```

* **`nonzero_samples` = 827 of 1034 = 80.0%** of the window, against window 1's
  7.4%.  This is the fraction item 2 of "What a valid re-run needs" asked for:
  the reading is about the pool, not about the instrument.
* Window: 2026-09-13 07:29:04 → 07:49:04 +01:00, `--interval 1`, opened on the
  first confirmed test binary rather than on the `cargo nextest run` launch —
  nextest compiles its test binaries after that launch, and a window opened
  there spends itself on compilation (a first attempt did exactly that: 37
  samples, all zero, and was discarded before it could be reported).
* Workload: `scripts/verify.sh test --scope all --profile both`, started in lane
  `_lane-34` for this measurement.  The window overlapped that gate's execution
  phase throughout — evidence from the gate's OWN log, which needs no pid
  attribution: it recorded `Starting 23359 tests across 662 binaries`, then
  `Summary [588.417s] 23359 tests run: 23359 passed (13 slow, 2 leaky), 67
  skipped`, then `Starting 9510 tests across 335 binaries` (the release pass) —
  588 s of debug-profile test execution plus the start of the release pass,
  inside a 1200 s window.  The `pgrep -af` snapshots taken at window open and
  close corroborate it: a `cargo nextest run --workspace` at open and a
  `cargo nextest run -p reify-constraints -p reify-eval …` release pass at close.
  (Those snapshot pids are not attributed here — they had exited by the time
  attribution could be checked, and an unverifiable attribution is worth less
  than the gate's own log.)
* **Sampling was LANE-SCOPED, not host-wide** — see "Why this window is not
  host-wide" below.  Windows 1 and 2 claim host-wide sampling; window 3 does
  not, and the difference is a property of who ran the sampler, not of the fix.

### The distribution, not just the peak

`peak` is a maximum, so record the shape it came from:

| confirmed test binaries | samples |
|---|---|
| 0 | 207 |
| 1–23 | 217 |
| 24–31 | 535 |
| exactly 32 | 28 |
| 34, 36, 44, 51 | 1 each |

The sustained plateau is 24–32 with a hard shoulder at **exactly 32**, the
generated `test-threads`.  Only **4 of 1034** samples exceed it, and the two
largest — 44 and 51 — are the window's FIRST TWO samples (`t=0s`, `t=1s`), taken
as the gate left its compile phase (the gate log's last line at window open was
`Compiling reify-eval`).

*Hypothesis (not measured):* those two samples caught nextest's test-LIST phase,
which executes each of the 662 test binaries to enumerate its tests, rather than
the execution phase `test-threads` bounds.  That would explain a burst above the
pool immediately after compilation and nowhere else in the window.

**The criterion does not rest on those two samples.**  Discarding every sample
above 32 still leaves `peak = 32 > 16`, so the verdict below holds either way.

### Why this window is not host-wide

The sampler confirms identity with `readlink /proc/<pid>/exe`, which requires
ptrace read access to the target.  Measured from the sandboxed agent that took
this window, on the same host and at the same time: **19 of 961** pid entries
resolved.  Every other process returned `Permission denied` on a link that
`ls -l` shows as `leo:leo` — including four LIVE `verify.sh` processes in other
lanes (pids 223138, 2361374, 2361376, 3578288), each re-checked as still present
in `/proc` at the moment of the probe, so the empty result is a denial and not a
vanished pid.  The caller reports `Uid 1000`, `CapEff 0000000000000000`,
`NoNewPrivs 1`.

*Hypothesis (not pinned down):* an LSM ptrace restriction attached to the
sandboxed agent role denies `PTRACE_MODE_READ_FSCREDS` outside the agent's own
domain.  Yama does not explain it (`ptrace_scope` is 1, and Yama's check returns
0 for any mode without `PTRACE_MODE_ATTACH`), and neither does DAC (the targets
share the caller's uid).

The consequence is what matters here: from a sandboxed agent the sampler can
only see its own process tree, so "sample beside a gate that is already running"
is unreachable and the only workable shape is to START the gate and sample its
descendants.  That is what was done, and it is why this window reports ONE
lane's gate rather than the host.  A sandbox-induced zero is a third member of
the silent-zero family this instrument's header already documents (the
cwd-induced zero and the never-saw-an-execution-phase zero), and the instrument
cannot yet tell it from the other two — filed as follow-up from esc-6375-2.

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
instrument work, not more sampling: tracked as **#6375**.

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

**Prefilter→confirm race (fixed, task 6375).**  This one is the hypothesis at
the end of "What this does and does not show", promoted to a measurement.  The
root cause has TWO halves, and fixing either alone leaves the defect:

1. *Per-candidate confirmation.*  The script forked one `readlink` per candidate
   AFTER the prefilter had returned.  Every fork widens the gap between "this
   pid was listed" and "this pid's exe was read", against tests that live
   0.2–0.8 s.
2. *The prefilter itself.*  `pgrep -f` walks argv across all of `/proc` and cost
   0.15–0.24 s on an idle-ish host (2.6 s per pass on the loaded window-1 host),
   so the list it returned had already decayed before confirmation began.
   Batching the confirmation of a stale list still confirms a stale list.

Measured A/B, taken first-hand during window 3's own execution phase, three
methods alternating within the same second (loadavg 88.61, 1075 processes):

| round | batched one-liner | fixed sampler | pre-fix algorithm |
|---|---|---|---|
| 1 | 29 | 23 | 13 |
| 2 | 24 | 24 | 10 |
| 3 | 21 | 29 | 6 |
| 4 | 27 | 18 | 6 |
| 5 | 25 | 21 | 12 |
| 6 | 24 | 25 | 7 |

The fixed sampler tracks an independent batched snapshot; the pre-fix algorithm
undercounts it by roughly 60%.  An earlier A/B on the same host at loadavg 94
gave 24/30/27/30/26 against 14/18/16/17/9 — a 35–65% undercount.

Fixed by making candidate discovery and confirmation ONE pass over ONE snapshot:
the default path enumerates `<PROC_ROOT>/*/exe` with a bash glob (readdir only,
no fork, no `pgrep`), and the whole candidate set is resolved by a single
multi-operand `readlink`.  Multi-operand `readlink` silently omits unresolvable
operands, so assert A5's "a vanished pid is skipped, not fatal" contract is now
satisfied structurally rather than by a per-pid guard.
`REIFY_SAMPLER_PIDS_CMD` survives as an override seam and feeds the same batched
confirmation, so the contract guard keeps pinning the code a real run takes.
Pinned by asserts B1 (a deletion tripwire: five candidates that vanish the
instant the first confirmation returns must still all count) and B2 (the default
path must discover from `PROC_ROOT`) in
`tests/infra/test_test_binary_concurrency_sampler.sh`.

**This one DID change the recorded numbers**, unlike the cwd fix above.  Window 3
is therefore **not** an apples-to-apples re-run of windows 1 and 2: it was taken
with a materially different instrument, and windows 1–2 must be read as floors.

## Host-wide framing (do not confuse with the per-run bound)

`test-threads` bounds **one** `cargo nextest run`, never the host.  With
`REIFY_TEST_SEMAPHORE_CONCURRENCY` at 1 (dark-factory-orchestrator.yaml:331,
dropped 2 -> 1 in df commit 712e6230d6) plus the merge role's bypass
(`scripts/lib_test_semaphore.sh:91`), the reachable steady state is
**2 × test-threads** — ~32 before task 6018, ~64 after.  A single lane's
observed peak is therefore expected at or below 32, not 64.

## What a valid re-run needs

**Tracked as #6375** — a live task, filed during task 6018's review-amendment
pass, covering both (a) the re-run over a full execution phase and (b) the
prefilter→confirm race fix.  Cited here deliberately and by number.  This
residual is the same one task 5984 carried and that "was tracked nowhere until
2026-08-05" (esc-5984-2); a prose "see the follow-up below" pointer naming no
task is how it went missing the first time, and nothing automated would have
caught it — reify-audit's PTODO gate excludes markdown from its sweep entirely
(`docs/prds/reify-audit-ptodo-detector.md` §6.8: swept extensions are
`.rs .ri .sh .py .ts .tsx .js`), so a stale pointer in a doc like this one is
invisible to it whether or not it carries a marker.  Citing a live task by
number is the only thing keeping this residual findable.  If #6375 is ever
closed without a conclusive window, this section — not the task — is the thing
to re-read.

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
