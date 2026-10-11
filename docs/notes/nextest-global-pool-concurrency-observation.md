# Observed test-binary concurrency after un-narrowing the global nextest pool

Task 6018 acceptance artifact (residual deliverable of task 5984 carry-over 1,
esc-5984-2).  Instrument: `scripts/sample-test-binary-concurrency.sh`
(contract guard: `tests/infra/test_test_binary_concurrency_sampler.sh`).

**Verdict: CONCLUSIVE, by window 3.  The acceptance clause IS discharged.**
The criterion is `nonzero_samples > 0` **AND** `peak > 16`; window 3 reports
`peak=51` with `nonzero_samples=827` of 1034 samples (80.0%), taken over a full
`scripts/verify.sh --scope all --profile both` execution phase with the
prefilter→confirm race fixed.  It holds with margin: discarding every sample
above the generated `test-threads` of 32 still leaves `peak=32 > 16`.

Two qualifications travel with that number and must travel with any citation of
it.  Window 3 is **one lane's gate, not the host** — see "Why this window is not
host-wide".  And windows 1 and 2 were taken with a materially different
instrument, so they are floors, not comparable re-runs.

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

## Window 1 — the first window that observed an execution phase

```
INFO: nextest test-binary concurrency: peak=14 samples=163 nonzero_samples=12 \
      interval=1s duration=420s deps_glob=*/target/*/deps/* host_nproc=32
```

The summary lines quoted for windows 1–3 predate the `own_uid_links=` and
`resolved_links=` fields that task #7500 appended to that line (see "Ptrace
blindness (detected, task #7500)" below), so none of them records its caller's
ptrace reach.

* Workload: `cargo nextest run --config-file <generated> -p reify-syntax
  -p reify-compiler -p reify-lsp -p reify-expr -p reify-core --no-fail-fast`,
  i.e. a **scoped debug-profile nextest pass**, not a full
  `scripts/verify.sh --scope all --profile both` gate.  The lane's `target/`
  was warm (83 GiB, 2824 built deps) and the branch diff touches no `.rs`
  file, so the pass went essentially straight to execution.
* The run reached test 3107/7579 during the window and was subsequently
  terminated by the harness `timeout 400` wrapper, not by a test failure.
* Sampling was meant to be host-wide (all lanes), not scoped to this lane.  The
  caller's ptrace reach was not recorded, because the fields that now report it
  did not exist yet.  Host-wide is therefore the instrument's intent for this
  window, not a verified property of it.
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
  7.4%.  This is the fraction item 2 of "What the valid re-run was" asked for:
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
  host-wide" below.  Windows 1 and 2 claim host-wide sampling (unverified, see
  window 1); window 3 does not, and the difference is a property of who ran the
  sampler, not of the fix.

### The distribution, not just the peak

`peak` is a maximum, so record the shape it came from.  Re-derived from the
window's raw per-sample stderr log (one `confirmed=` line per sample) rather than
retyped — that log is transient, under `/tmp`, so this table is the durable
record of it:

| confirmed test binaries | samples |
|---|---|
| 0 | 207 |
| 1–23 | 260 |
| 24–31 | 535 |
| exactly 32 | 28 |
| 33–51 | 4 |
| **all** | **1034** |

The bins are contiguous and exhaustive, and they reconcile with the summary line
at the top of this section on both of its fields: they sum to 1034 = `samples`,
and the four nonzero bins sum to 827 = `nonzero_samples`.  That reconciliation is
the point of stating the total — it makes a future transcription slip in this
table self-detecting.  (The `1–23` cell read 217 as first published, which is
exactly the kind of slip meant; corrected here from the log.)

In the top bin the only values observed were **34, 36, 44 and 51, one sample
each** — no sample landed on 33.

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

The mechanism, first recorded here as a hypothesis, has since been pinned under
task #7500.  The cause is Landlock's domain-scoped ptrace restriction: the
sandboxed agent runs inside a Landlock domain, and that domain denies ptrace read
access to every process outside it, whatever that process's uid.  Yama and DAC
are excluded.  For the measurements, see "Ptrace blindness (detected, task
#7500)" under "Instrument defects found after the fact".

The consequence is what matters here: from a sandboxed agent the sampler can
only see its own process tree, so "sample beside a gate that is already running"
is unreachable and the only workable shape is to START the gate and sample its
descendants.  That is what was done, and it is why this window reports ONE
lane's gate rather than the host.  A sandbox-induced zero is a third member of
the silent-zero family this instrument's header already documents (the
cwd-induced zero and the never-saw-an-execution-phase zero).  Since task #7500
(filed from esc-6375-2) the instrument tells it apart from the other two.  Its
summary line reports `own_uid_links=` and `resolved_links=`, and it WARNS when
fewer than half of the caller's own-uid candidates resolved.

## Why window 1 was not conclusive (retained as provenance)

The acceptance criterion is `nonzero_samples > 0` **AND** `peak > 16`.
Window 1 satisfies the first and fails the second, so it is **not** a
conclusive observation, and `peak=14` must **not** be reported as "the pool
stayed under 16".  Window 3 is the observation that discharges the clause; this
section records why a second attempt was needed at all.

The reason is instrument coverage, not a bound:

* Only **12 of 163 samples (7.4%)** saw any confirmed test binary at all, even
  though the run was executing continuously throughout the window.
* Each pass cost ~2.6 s wall (163 samples in 420 s) because the host was loaded
  by the run being measured — well above the 0.28 s measured on an idle host.
* The tests in flight were short: the observed per-test times were ~0.2–0.8 s.

When this section was first written it closed on a hypothesis: that the
prefilter→confirm gap systematically undercounts.  *That hypothesis has since
been MEASURED and CONFIRMED* (task 6375; see
"Prefilter→confirm race" under "Instrument defects found after the fact").  At
this churn rate most candidates returned by the `pgrep -f` prefilter had already
exited by the time `readlink /proc/<pid>/exe` confirmed them, and the prefilter
itself — 0.15–0.24 s idle, 2.6 s per pass on this loaded host — was a second,
independent source of staleness.  Measured undercount: 35–65%.  `peak=14` is
therefore a **floor**, not a ceiling, and this section's reasoning about window 1
stands as written.  Window 3 is the conclusive reading; this section is retained
as the provenance of how it was reached.

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
the end of "Why window 1 was not conclusive", promoted to a measurement.  The
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
gave 24/30/27/30/26 against 14/18/16/17/9 — a 35–65% undercount.  For cost: a
batched whole-`/proc` pass took 0.04/0.04/0.22 s there, against 0.15–0.24 s for
the `pgrep` prefilter *alone* with a fork per candidate still to come on top
(2.6 s per pass on the loaded window-1 host), which is what supersedes the old
affordability objection to a whole-`/proc` scan.

**This section is the single owner of those numbers.**  Both
`scripts/sample-test-binary-concurrency.sh` and the B1 assert in
`tests/infra/test_test_binary_concurrency_sampler.sh` cite it for the 35–65%
conclusion rather than restating the series — three copies of one measurement had
already drifted over which one was authoritative.  Re-derivations belong here.

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

**Ptrace blindness (detected, task #7500).**  This is the third silent zero.
`readlink /proc/<pid>/exe` needs ptrace READ access to `<pid>`.  A caller that
lacks it gets an empty confirmation for every process outside its own tree, and
so reports `peak=0 nonzero_samples=0` over a host that may be busy running tests.
That reading is textually identical to window 2's honest INCONCLUSIVE.  This
section is the single owner of the measurements below; the sampler's header and
the contract guard cite it rather than restating them.

Measured, each in a named context:

1. *Task 6375's lane* (`_lane-34`, HEAD `28eab60f8a`).  The caller was a
   sandboxed agent shell: Uid 1000, CapEff 0, NoNewPrivs 1, Seccomp 0, Yama
   `ptrace_scope` 1.  `printf '%s\0' /proc/[0-9]*/exe | xargs -0 readlink`
   resolved **6 of 1099** glob entries, and those 6 were the agent session's own
   process tree.  Every same-uid non-descendant sampled returned `Permission
   denied` on a link that `ls -l` shows as `leo:leo`.  Eight `verify.sh`
   processes were in flight at the time, two of them with a live `cargo-nextest
   nextest run`, yet the sampler reported `peak=0 nonzero_samples=0` over a
   40 s window.  Window 3's agent measured 19 of 961 the same way (see "Why
   this window is not host-wide").
2. *Architect probe, 2026-10-11, `_lane-24`*: the same host in two contexts.

   | context | own-uid `/proc/<pid>` | of which resolved | other-uid | of which resolved |
   |---|---|---|---|---|
   | unsandboxed shell: uid 1000, NoNewPrivs 0, AppArmor `unconfined` | 560 | 549 | 837 | 0 |
   | same probe under `gui/src-tauri/sandbox/landlock_exec.py` (NoNewPrivs 1) | 548 | 1 | not recorded | not recorded |

   Unsandboxed, the 11 own-uid misses are non-dumpable own processes
   (`ssh-agent`, `gpg-agent`, `systemd --user`, `(sd-pam)`, …) or zombies, and
   667 of the 730 root-owned entries are kernel threads.  Under the helper,
   `xargs -0 readlink -v` over every `/proc/*/exe` reported 1481 `Permission
   denied` and 1 `No such file`.

**Mechanism (pinned).**  Since Landlock ABI 1, a process inside a Landlock
domain may use ptrace, `PTRACE_MODE_READ` included, only on processes in its own
domain or a nested one, whatever their uid.  That reproduces 6375's signature
exactly: the caller's own tree resolves, and same-uid non-descendants get
`EACCES`.  Two other causes are excluded:

* Yama: `ptrace_scope` 1 restricts only `PTRACE_MODE_ATTACH`.
* DAC: the targets share the caller's uid.

The property the fix rests on is that a **stat of `/proc/<pid>` is not
ptrace-gated**: 548 own-uid directories were still counted under the helper.  So
a blind caller can still COUNT the own-uid processes it cannot read.

**Why the denominator is own-uid.**  A naive resolved/enumerated ratio is about
40% on a HEALTHY unsandboxed non-root host: 595 of 1411 in the architect's
context.  DAC denies every root daemon and kernel thread to such a caller, so a
threshold on that ratio would sit right next to normal operation.  Restricted to
candidates the caller's uid owns, the ratio is 549/560 = 98% healthy against
1/548 = 0.2% under Landlock, a gap of two orders of magnitude.

**Fix.**  The summary line gains two appended fields, both summed over the
window:

* `own_uid_links=`: candidates whose `/proc/<pid>` directory the caller's
  effective uid owns, tested with the builtin `[ -O ]`.  That test is a stat of
  the directory, never of the exe link.
* `resolved_links=`: operands that the batched `readlink` resolved.

When `own_uid_links > 0` and `2 × resolved_links < own_uid_links`, a stderr
WARNING says the reading covers only the caller's visible tree.  In that case a
zero is not evidence of an empty host, and a non-zero peak is a floor.  The exit
status stays 0, and this WARNING is independent of the `nonzero_samples=0` one.

Pinned by asserts in `tests/infra/test_test_binary_concurrency_sampler.sh`:

* C1, C1b, C2 and C3 are hermetic.  C1 is partial blindness, C1b total
  blindness, C2 a few own-uid denials that must not warn, and C3 checks that the
  grammar change is additive.
* C4a runs against the real `/proc` and checks that other-uid DAC denials stay
  out of the denominator.
* C4b runs under real Landlock via the vendored helper: 8 spawned processes
  resolve 8/8 directly and 0/8 inside a nested domain.

Accepted limitation: for a uid-0 caller, root-owned kernel threads enter the
denominator and never resolve.  A near-idle host could therefore false-warn
there.  Nobody runs the instrument that way.

*Demonstration, and its own context.*  These lines were taken from this task's
implementer shell: `_lane-24`, HEAD `dfd7995d5b`, 2026-10-11 19:53 +01:00, Uid
1000, CapEff 0, NoNewPrivs 1, Seccomp 0, AppArmor `unconfined`, loadavg 82.54.
That shell is itself **sandboxed**, so the bare run is the blind case and NOT a
host-wide reading:

```
# bare, from the implementer's own Landlock domain
INFO: nextest test-binary concurrency: peak=0 samples=1 nonzero_samples=0 \
      interval=0s duration=0s deps_glob=*/target/*/deps/* host_nproc=32 \
      own_uid_links=706 resolved_links=8
# nested once more under gui/src-tauri/sandbox/landlock_exec.py
INFO: nextest test-binary concurrency: peak=0 samples=1 nonzero_samples=0 \
      interval=0s duration=0s deps_glob=*/target/*/deps/* host_nproc=32 \
      own_uid_links=699 resolved_links=4
```

Both runs emitted the blindness WARNING alongside the `nonzero_samples=0` one.
Their `peak=0` says nothing about whether test binaries were running elsewhere
on the host, which is the point of the fields.  No unsandboxed reading was
available from this context; the architect's unsandboxed probe above is the
healthy-side reference.

## Host-wide framing (do not confuse with the per-run bound)

`test-threads` bounds **one** `cargo nextest run`, never the host.  With
`REIFY_TEST_SEMAPHORE_CONCURRENCY` at 1 (the `REIFY_TEST_SEMAPHORE_CONCURRENCY` key in dark-factory-orchestrator.yaml `verify_env`,
dropped 2 -> 1 in df commit 712e6230d6) plus the merge role's bypass
(the merge bypass in `test_semaphore_acquire`, `scripts/lib_test_semaphore.sh`), the reachable steady state is
**2 × test-threads** — ~32 before task 6018, ~64 after.  A single lane's
observed peak is therefore expected at or below 32, not 64.

**Window 3 confirms this for the execution phase, and bounds the exception.**
That window is a single lane's gate, and its distribution has a hard shoulder at
exactly 32 with 1030 of 1034 samples at or below it.  Four samples exceeded it
(34, 36, 44, 51), so "at or below 32" is a statement about the execution phase
and not an invariant over every instant of a gate; the two largest excursions
were the window's first two samples, as the gate left compilation.  See window
3's distribution table for the hypothesis about what those caught.

## What the valid re-run was (done — task #6375)

**Tracked as #6375** — filed during task 6018's review-amendment pass, covering
both (a) the re-run over a full execution phase and (b) the prefilter→confirm
race fix.  Cited here deliberately and by number, and retained now that it is
done, because this residual is the same one task 5984 carried and that "was
tracked nowhere until 2026-08-05" (esc-5984-2).  A prose "see the follow-up
below" pointer naming no task is how it went missing the first time, and nothing
automated would have caught it — reify-audit's PTODO gate excludes markdown from
its sweep entirely (`docs/prds/reify-audit-ptodo-detector.md` §6.8: swept
extensions are `.rs .ri .sh .py .ts .tsx .js`), so a stale pointer in a doc like
this one is invisible to it whether or not it carries a marker.

What each requirement asked for, and what window 3 delivered:

1. *A window covering a full `scripts/verify.sh --scope all --profile both`
   execution phase, not a scoped 5-crate pass.*  **Done** — 588 s of
   debug-profile execution across 662 test binaries plus the start of the
   release pass, inside a 1200 s window.
2. *`nonzero_samples` at a usable fraction of `samples`.*  **Done** — 827/1034 =
   80.0%, against window 1's 7.4%.  The single-digit-percent failure mode this
   item warned about did not recur.
3. *An instrument fix for the prefilter→confirm race.*  **Done** — and it needed
   BOTH halves: batching the confirmation into one multi-operand `readlink` AND
   dropping the `pgrep` prefilter, because confirming a list that a slow
   prefilter already made stale still confirms a stale list.  Pinned by asserts
   B1 and B2.
4. *No constraint on **where** the sampler is launched from* — it is a host-wide
   instrument and its result is cwd-independent *as of* the fix described under
   "Instrument defects found after the fact".  **This warning still stands and
   still applies:** any reading taken with an **earlier copy** of the script must
   have its cwd checked before it is trusted (if that cwd contained a tree
   matching the run's `deps_glob`, the reading is a silent zero and is not
   evidence about anything).  Such a reading must also have its **caller's
   ptrace reach** checked, since a sandboxed caller sees only its own process
   tree and produces the same shape of silent zero.  Since task #7500 that
   second check is mechanical: readings from the current instrument carry
   `own_uid_links=` / `resolved_links=` and WARN when the caller was blind.
   Only older readings, windows 1–3 among them, still need it checked by hand.

The un-narrowing is now justified by an **observed** peak — window 3's
`peak=51`, with a sustained execution-phase plateau whose shoulder sits exactly
at the generated `test-threads = 32` — and no longer only by the config being
read back as `test-threads = 32`.

**The one residual.**  The instrument cannot yet distinguish a sandbox-induced
zero from the two silent zeros it already documents.  That does not affect window
3 (which is non-zero throughout and states its own scope), and it is filed as
**#7500** (filed from esc-6375-2) rather than left as prose here — the same
cite-a-live-task-by-number discipline this section argues for above, applied to
the residual this round leaves behind.
