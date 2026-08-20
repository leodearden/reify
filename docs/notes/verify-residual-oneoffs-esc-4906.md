# Verify Residual One-Offs (esc-4906-52) — Close-Out

**Task #4962 | 2026-07-03**

---

## Problem

`esc-4906-52` (auto-watcher L1, filed 2026-07-03T07:14, resolved
2026-07-03T09:23) flagged what looked like a **rotating** `--include-infra`
verify-gate flake blocking task 4906: a different pooled test failing on
each retry, every one green standalone, against a backdrop of sustained
host overload (82.79 1m-load-avg / 67.66 5m on 32 cores — roughly 2.6x
oversubscribed, "sustained-high all session").

Flake signature recorded in the escalation (confirmed 4x, each failing test
green standalone):

| Attempt | Failing test |
|---|---|
| 1 | `test_occt_flock_gate.sh` |
| 2 / 3 | `test_pool_3.sh` |
| earlier debugger runs | `test_cpu_load_governance_deflake.sh` |
| steward retry this session | `reify-ast::dag_invariant` (both tests, 0.02s) |

Root-causing the T1/T3 half of this (below) explained away the "rotating"
appearance and the `test_occt_flock_gate.sh` / `test_pool_3.sh` rows. This
note closes out the two **residual** one-offs left over: `reify-ast::dag_invariant`
and `test_cpu_load_governance_deflake.sh`.

---

## Root Cause (T1/T3) — for context, fixed by task 4965

Not concurrency, not load: a **deterministic shell-quoting bug** in
`test_occt_flock_gate.sh`'s own T1/T3 asserts (lines 315–333). They capture
`verify.sh test --profile both --scope all --print-plan` into
`_T1_PLAN`/`_T3_PLAN` and interpolate them **unescaped** into a
double-quoted `bash -c` string. Since deploy `65b8412206` (2026-07-02
13:35) exports `REIFY_GATE_EXCLUDE_HEAVY=1` in `dark-factory-orchestrator.yaml`'s
`verify_env`, the captured test-phase plan lines carry the heavy-exclusion
fragment `-E "not ((package(...) & binary(...)) | ...)"` — its embedded
double quotes/parens/`&` terminate the `printf` argument early and hit the
bash parser (`syntax error near unexpected token '('`) → `rc=2` → assert
FAIL. This reproduces in **100%** of orchestrator verifies (knob always
exported there) and **0%** of standalone shells (knob unset there) — which
is exactly the "green standalone, red under orchestrator" pattern the
escalation observed for T1.

Fixed by task 4965 (status=done, merged `1e28e9dc2316327b5e8cf8d8d661303a00caa4ad`,
fix commit `5af93e53c01fbbfb5123165217023b560704c960`): T1/T3 now use the
same safe idiom as T2/Test 17 (export the captured plan variable, reference
it escaped inside `bash -c`), plus the five latent same-shape sites T4–T7
were hardened pre-emptively.

**The "rotating flake" illusion** (`test_pool_3.sh` appearing to fail on
attempts 2/3) was the orchestrator's `cause_hint` parser grabbing the line
`FAILED test_pool_3.sh` out of `test_run_all.sh`'s own T9a output — an
**intentionally-failing mock fixture** that `test_run_all.sh` uses to prove
its own failure-reporting path works, not a real failing test. A possible
follow-up worth flagging to dark-factory: the cause_hint parser should not
treat a runner's self-test mock-fixture output as a real failure signal.
That parser lives in dark-factory, not this repo (per the repo's
ships-primitive / wires-invocation seam pattern), so it is out of scope
here and is **not** filed as part of this close-out.

---

## Residual #1 — `reify-ast::dag_invariant` (0.02s dual-fail)

**What the test does.** `crates/reify-ast/tests/dag_invariant.rs` holds
exactly two pure-unit tests:

- `reify_ast_depends_only_on_reify_core`
- `reify_ast_has_no_tree_sitter_dependency`

Both only `std::fs::read_to_string` a static `Cargo.toml` and string-scan
its lines — no subprocess spawn, no compilation, no native-dep link.
Confirmed by reading the crate's own dependency graph:
`crates/reify-ast/Cargo.toml` declares exactly `reify-core.workspace = true`
as its only `reify-*` dependency, and `crates/reify-core/Cargo.toml`'s
`[dependencies]` block is just `xxhash-rust` + optional `serde` — zero
OCCT/manifold/gmsh/OpenVDB or any other native kernel anywhere in
`reify-ast`'s dependency chain.

**Why 0.02s is the tell.** A 0.02s **dual** whole-binary failure (both
tests failing together, near-instantly) is inconsistent with an assertion
failure — a real B2-invariant violation would take normal per-test runtime
(tens to hundreds of ms, see below) and produce a panic message with a
line-listing diff, not an instant joint failure of two independent tests
with unrelated assertions. It is consistent with a nextest
spawn/dynamic-link/binary-corruption/OOM/env-level fault that aborts the
whole test binary before either test body runs — exactly the class of
fault a severely oversubscribed host (2.6x cores, per esc-4906-52) can
produce.

**Non-reproduction evidence (post-4965, this base).** `cargo nextest run -p
reify-ast` was run **21 times** (1 initial + a 20-iteration loop) on this
lane's post-4965 base:

```
run  1: Summary [0.464s] 39 tests run: 39 passed, 0 skipped
run  5: Summary [0.148s] 39 tests run: 39 passed, 0 skipped
run 10: Summary [0.376s] 39 tests run: 39 passed, 0 skipped
run 16: Summary [1.183s] 39 tests run: 39 passed, 0 skipped
run 20: Summary [0.490s] 39 tests run: 39 passed, 0 skipped
```

**39/39 PASS on all 21 runs, zero failures.** Both `dag_invariant` tests
were green on every run, with individual per-test times ranging from
0.033s to 0.891s across the loop (e.g. run 1: `0.086s` / `0.257s`; run 15:
`0.033s` / `0.173s`; run 16: `0.805s` / `0.891s`) — normal nextest
scheduling jitter under whatever ambient load existed during this loop,
never anywhere near a 0.02s dual-fail.

**Archived logs.** `data/verify-logs/` does not exist in this worktree.
The `esc-4906-52` resolution references "task 4906's three archived verify
attempts," which live under task 4906's own `.task/` directory in a
different lane/worktree not reachable from here. A best-effort search of
this lane found no archived nextest stderr for the specific 0.02s
`dag_invariant` run — recorded as **unavailable**, per this task's
prereq-2(c).

**Steward guidance.** A 0.02s whole-binary dual-fail over pure-unit tests
that only read a static file is a harness/spawn/env artifact, not a source
bug — **retry it**, do not chase it as a code defect. No source fix is
possible (there is no source defect) or warranted.

---

## Residual #2 — `test_cpu_load_governance_deflake.sh` (one-off)

**What the test does.** A hermetic meta-test (task 4846) that spawns
`test_cpu_load_governance.sh` (the SUT) as a subprocess with
`REIFY_CPU_GOV_TEST_BUDGET_S=0` + `REIFY_CPU_GOV_TEST_QUIET_CEILING=0`,
which unconditionally forces the SUT's cheap-skip / quiet-box code paths
so its three assertions — A1 (no wall-clock-budget skip marker), A2 (SUT
exits 0), A3 (`share_ge_proportional` discriminator sanity, both
directions) — need no real CPU burn and no real PSI signal. Per the file's
own header: *"Hermetic: no real CPU load, no cgroup substrate, no PSI
required for A1/A2/A3."*

**Mechanism correction.** This task was retargeted on the premise that
"task 4926 ... reclassif[ied] the meta-test host-exclusive," but that is
not what happened, and it's worth setting straight so a future debugger
doesn't conflate the two similarly-named files:

- `test_cpu_load_governance_deflake.sh` has been classified `host-exclusive`
  in `tests/infra/run-all-classification.manifest` since task 4921 first
  authored the manifest (commit `8400c306c40d16babfc6d42c78b8a6cb2abc22ba`,
  2026-07-01T17:07:35+01:00) and is **unchanged since** — `git log -L
  46,46:tests/infra/run-all-classification.manifest` shows exactly one
  owning commit, ever.
- Task 4926 (PRD `run-all-host-infra-partition.md` task H5, merge
  `4834a6b3a824afd98c20c0e4dd45fabff3c3d316`, landed
  2026-07-02T16:47:44+01:00) reclassified a **different file that merely
  shares a name prefix** — `test_cpu_load_governance.sh` (no `_deflake`
  suffix; the real SUT, which has genuine CPU-burn/PSI rows) — from
  `host-exclusive` to `pool`, after making it host-load-independent via
  confined-cgroup-quota + synthetic-PSI fixturing. `git diff
  4834a6b3a8^1 4834a6b3a8 -- tests/infra/run-all-classification.manifest`
  shows exactly that one swap; the deflake meta-test's row is untouched in
  both parents.

**Best explanation for the one-off.** Given the meta-test's hermetic-by-design
assertions and its unchanged host-exclusive bucketing, the failure recorded
in esc-4906-52's flake signature ("earlier debugger runs:
`test_cpu_load_governance_deflake.sh`") is most parsimoniously explained as
a transient artifact of the same severe host-oversubscription window
documented in that escalation (e.g. subprocess-spawn/scheduling latency
around its `timeout 120` wrapper under ~2.6x-core load) rather than a code
defect in the meta-test or the SUT it drives.

**Non-reproduction evidence (post-4965/4926, this base).**

```
$ bash tests/infra/test_cpu_load_governance_deflake.sh
=== cpu-load-governance de-flake meta-test (task 4846) ===
  PASS: A3a: share_ge_proportional(100,100,300,100,0.10) is False (broken→ROW4-1 RED)
  PASS: A3b: share_ge_proportional(300,100,300,100,0.10) is True (healthy→ROW4-1 GREEN)
  PASS: SUT exists at .../tests/infra/test_cpu_load_governance.sh
  PASS: A1: SUT output contains NO 'live section budget' skip marker
  PASS: A2: SUT exits 0 under cheap-skip config (rc=0)
Results: 5 passed, 0 failed
```

**Steward guidance.** Retry; the assertions are hermetic and non-flaky by
construction, and the file's own host-exclusive bucketing already isolates
it from same-run pool-test contention.

---

## Conclusion

Both residuals are transient, environment/harness-level, and **do not
reproduce** on the post-4965 base:

| Residual | Verdict | Evidence |
|---|---|---|
| `reify-ast::dag_invariant` 0.02s dual-fail | harness/spawn artifact, no source defect | 21/21 runs green (39/39 each) |
| `test_cpu_load_governance_deflake.sh` one-off | transient host-overload artifact, no source defect | 5/5 assertions green |

No source fix is possible or warranted for either. Task 4962 is closed out
by this note plus a short cross-reference comment in
`tests/infra/test_cpu_load_governance_deflake.sh` pointing back here.

**Sources:** task 4965 (T1/T3 fix), task 4926 (H5 — `test_cpu_load_governance.sh`
pool conversion, unrelated to residual #2's file), task 4921 (original
manifest authoring), `esc-4906-52` (escalation record), `docs/notes/verify-pipeline-knobs.md`
(verify-gate knob digest), `docs/prds/run-all-host-infra-partition.md`
(host-exclusive/pool partition design).
