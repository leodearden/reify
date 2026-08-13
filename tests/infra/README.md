# tests/infra/

Shell meta-tests for reify's infrastructure scripts (`scripts/lib_portable.sh`,
`scripts/tree-sitter-generate.sh`, `scripts/test_pm_standardization.sh`, etc.).

## Auto-discovery

`run_all.sh` discovers and runs every file matching `test_*.sh` in this directory.
To add a new meta-test, create `test_<name>.sh` — it will be picked up automatically
on the next `run_all.sh` invocation and in CI.

**Exception:** `test_helpers.sh` is a shared library, not a test runner.
It is excluded from discovery by exact name.

**Mandatory, mechanically-gated: a `run-all-classification.manifest` row.**
Every `test_*.sh` here also needs a `<test_basename> <bucket>` row in
[`run-all-classification.manifest`](run-all-classification.manifest) —
`bucket` is one of `pool`, `intra-run-serial`, or `host-exclusive`; see that
file's header comment for the definition of each. `scripts/check-infra-
classification-manifest.sh` runs as `scripts/verify.sh`'s **first** plan
entry whenever `RUN_RUST=1` and fails the build in *both* directions: a
`test_*.sh` on disk with no manifest row, or a manifest row with no backing
file. **Add the test file and its manifest row in the same commit** — because
the gate fails both ways, splitting them across commits leaves an
intermediate commit red, and the failure surfaces at that first gate entry,
far from this README. The executable bit is *not* required: every runner
path invokes the file via `bash <file>`, and the repo is a genuine mix today
(131 tracked files at mode `100755`, 41 at `100644`).

## Shared test helpers

All test files (except `test_tree_sitter_pipeline.sh`, see below) source
`test_helpers.sh` for the `assert()` / `test_summary()` pattern:

```bash
source "$SCRIPT_DIR/test_helpers.sh"

assert "my condition" test -f "$SOME_FILE"
# ...
test_summary   # exits 0 if all passed, 1 if any failed
```

`test_tree_sitter_pipeline.sh` uses its own richer assert API (colored output,
`assert_cmd_success` / `assert_cmd_fails`, trap-based cleanup) and is intentionally
excluded from the shared module.

## CI wiring

`run_all.sh` is wired into `dark-factory-orchestrator.yaml`'s `test_command` via:

```
if test -f tests/infra/run_all.sh; then bash tests/infra/run_all.sh; fi
```

This guard pattern matches the convention used for `tests/sync_comments_test.sh`.
The `sync_comments_test.sh` entry is kept separate because that script lives in
`tests/` (not `tests/infra/`) and is not auto-discovered by `run_all.sh`.

## Wall-clock upper-bound guard (`wallclock:allow`)

`test_no_new_wallclock_upper_bounds.sh` is a **static-grep regression guard**
(task #4848, PRD `infra-test-wallclock-deflake.md` T9).  It scans every
`tests/infra/*.sh` for wall-clock absolute-upper-bound assertions of the form:

```
assert "... within Ns ..." test "$elapsed_var" -le N
```

A line is flagged iff all of: (1) `assert`-wired, (2) upper-bound operator
(`-le` / `-lt`), (3) a genuine **time-measurement signal** — a time-suffixed
variable operand (`ELAPSED` / `_S` / `_MS` / `_NS` / `SECONDS`) **or** a
description lexeme (`elapsed`, `within Ns`, `duration`), (4) no inline
`wallclock:allow` escape.

Task #5257 tightened condition (3): bare description prose `wall` or `seconds`
alongside a literal or config-constant operand (e.g. a `-lt 3600` pass-level
ceiling extracted from `nextest.toml`) no longer matches — such a line carries
no time-measurement signal.  Only the lowercase `seconds` free-prose lexeme was
dropped; because bash `[[ =~ ]]` is case-sensitive the uppercase `SECONDS`
*variable suffix* is independent and retained, so a `$wait_SECONDS -le 30`
operand is still flagged.

### Opting out: `wallclock:allow`

If a generous anti-hang guard is **legitimately wall-clock** but
**non-flaky by design** (the test is discriminated by something other than
elapsed magnitude — e.g. exit code, stderr pattern, or a boolean marker),
annotate the asserting line:

```bash
assert "exits within 10s (generous anti-hang)" \
    test "$_ELAPSED" -le 10 # wallclock:allow — <reason>
```

The `# wallclock:allow` token on any physical line of the logical assert
tells the guard to skip it.  The reason should cite WHY the wall-clock
magnitude is load-safe (exit code, marker, etc.) so the exemption is
auditable.

**Current blessed survivors** (as of task #5257):
- `test_occt_flock_gate.sh` Tests 14 & 22: exit-75 + stderr pattern (`_ELAPSED*` operand)
- `test_find_uses_smoke_runner.sh` ARM A liveness guard: rc!=0 + the `E2E_SMOKE_LAUNCHER_DEATH phase=readiness` marker (`_t4_elapsed` operand). Since task #5596 this assert runs once per e2e smoke runner (six in total), all sharing that one operand name and annotation.
- `test_lane_x_flock.sh` flock-timing guard (`_ELAPSED18_MS` operand)

These four retain their `wallclock:allow` escapes: each carries a real
`elapsed` / `ELAPSED` / `_MS` time-measurement signal and is still (correctly)
flagged.  The six `nextest` / `occt` config-constant `-lt 3600` pass-level
ceilings that task #5257 de-annotated are **not** survivors — after the
condition-(3) tightening they carry no time-measurement signal and pass
un-flagged without an escape.

## Opt-in soak: seed lane-lock release (`REIFY_RUN_SEED_LANE_LOCK_SOAK`)

`test_seed_lane_lock_release_soak.sh` is the repeat-N characterization
instrument for `scripts/seed-warm-lane.sh`'s lane-lock release-at-exit
property (task #5705).  It runs in two layers:

- **Always** — a sub-second hermetic positive/negative control proving the
  `flock -n` probe primitive reports HELD on a held lock and FREE on a free
  one.  Sited *before* the opt-in gate, so it is never dead code.
- **`REIFY_RUN_SEED_LANE_LOCK_SOAK=1`** — drives N real seed invocations under
  `cpu_load_fixture.sh` contention (waiting for the load workers to be up and
  demonstrably burning CPU first, so the ramp is never measured as idle),
  probing the lane lock the instant seed's own parent exits, and prints one
  structured line:

  ```
  SEED_LANE_LOCK_SOAK held=<n> iters=<N> workers=<w>
  ```

  `REIFY_SEED_LANE_LOCK_SOAK_ITERS` sets N (default 50); workers are
  `min(nproc, 24)`.  Measured on a 32-core host: ~0.39s per iteration once the
  load is established, so the default is ~18s and `ITERS=200` ~78s.

The assertion is `held == 0` — a **count**, never an elapsed time, so it cannot
become one of the absolute wall-clock upper bounds the guard above rejects.

The mechanism, the argument for `flock -u` over `exec 9>&-`, and the measured
rates are **deliberately not repeated here**: they live in exactly one place,
the `LANE-LOCK RELEASE CONTRACT` block at the flock acquire in
`scripts/seed-warm-lane.sh`.  Re-measuring is the whole point of this harness,
and a copy in this file would be one more site to update in lockstep.

The permanent regression guards are hermetic and live in
`test_seed_warm_lane.sh` — `H4b`/`H4e` (structural release marker) and `H7b`
(an inherited FD 9 is never unlocked), plus **`H4c`**, which forks a live
holder of a dup of seed's lane-lock FD and asserts the lock reads back free
anyway; that last one is the only guard that fails if the release runs but has
no effect.  This soak is the instrument for re-measuring the rate if the
property ever resurfaces, which is why it is default-skipping and classified
`host-exclusive`.

## Repo-wide guard: a lock held across a detached fork

`test_flock_detached_fork_guard.sh` scans **every tracked shell script**
(`git ls-files -- '*.sh' 'hooks/*'` — 247 files today, including the
extension-less git hooks) and flags one shape: a script that acquires a `flock`
on a numbered FD **it opened itself**, forks a **detached** background child
(`... &`) downstream of that acquire, and never releases with an explicit
`flock -u <fd>`.  The child inherits the descriptor, so the lock outlives the
parent and the next consumer blocks on a holder that has already exited.

Zero offenders exist today.  The guard exists because the class has bitten
twice (2026-04-20 sccache/FD-9 wedge; 2026-07-28 `seed-warm-lane.sh`, fixed by
task #5705), and a static scan is the only thing that makes a third recurrence
loud instead of silent.  It is a **pure text predicate** — no stat, no network,
no host state — which is why it sits in the hermetic `pool` bucket.

**Two shapes are exempt, and both exemptions are structural rather than
allow-listed**, because flagging either would be actively harmful:

- **Foreground children.**  A parent that runs its child in the foreground
  holds the lock for exactly as long as the child runs and reaps it before
  exiting; telling it to release would push it to drop a lock it is still
  legitimately using.  Only a line whose *final effective token* is a bare `&`
  counts as a fork, so `"$@" 9<&-` (`scripts/lib_test_semaphore.sh`) can never
  enter the candidate set.
- **Inherited FDs.**  An unguarded `flock -u 9` where FD 9 came from the caller
  releases the **caller's** lock — strictly worse than the bug.  A candidate
  requires a *local* open, so a file that merely inherits the descriptor is
  never considered and the guard is incapable of advising a release it has no
  right to advise.  (Whence `scripts/seed-warm-lane.sh` installs its release
  trap *inside* the branch that opened the FD; `test_seed_warm_lane.sh`'s `H7b`
  pins that.)

The all-clear is only worth as much as the instrument, so three liveness
controls ship with it: a floor on the corpus size, membership checks for known
files in both trees, and a **mutation control** that deletes the one release
statement from the real `seed-warm-lane.sh` and demands the result flags — at
full scale, prose and all.  "Zero offenders" is therefore a measurement, not
the silence of a broken scan.

**SELF-MATCH SAFETY.**  The guard scans its own file.  Fixture bodies carrying
the offending shape are therefore assembled from shell variables at runtime and
written only into a `mktemp -d` dir, never emitted as literal source lines (the
same convention `test_no_new_wallclock_upper_bounds.sh` and
`test_reify_audit_ptodo.sh` use).  A dedicated assertion pins that the file
scans clean.

**Honest limitation.**  A file-local syntactic scan cannot follow FD provenance
across `source`.  `run_all.sh` holds the Lane-X FD 9 opened inside the sourced
`scripts/lib_lane_x_flock.sh` and forks pool workers with `) &`; it contains no
local open, so it is outside the guard's stated shape.  It is independently
safe — each worker runs `bash ... 9<&-` and closes with `exec 9>&-`.  Chasing
provenance across `source` would need interprocedural analysis and would
forfeit the host-independent, no-stat property above.

The mechanism, the argument for `flock -u` over `exec 9>&-`, and the measured
held-after-exit rates are **not repeated here** — same reason as the soak
section above: they live in the `LANE-LOCK RELEASE CONTRACT` block at the flock
acquire in `scripts/seed-warm-lane.sh`.

## Files

| File | Purpose |
|------|---------|
| `run_all.sh` | Discovery runner — runs all `test_*.sh` files |
| `test_helpers.sh` | Shared library: `assert()` and `test_summary()` |
| `test_flock_detached_fork_guard.sh` | Regression guard: a locally-opened flock FD held across a detached `&` fork with no `flock -u` release |
| `test_no_new_wallclock_upper_bounds.sh` | Regression guard: static-grep for new wall-clock upper-bound asserts |
| `test_npm_ci_hardening.sh` | Tests npm ci guard conventions in dark-factory-orchestrator.yaml |
| `test_portable_sha256.sh` | Tests `portable_sha256()` from `scripts/lib_portable.sh` |
| `test_portable_timeout.sh` | Tests `portable_timeout()` from `scripts/lib_portable.sh` |
| `test_release_mode_in_test_command.sh` | Tests dark-factory-orchestrator.yaml runs cargo test --release for release-only tests |
| `test_run_all.sh` | Tests this `run_all.sh` discovery runner |
| `test_setup_worktree_debug_port.sh` | Tests `allocate_free_port()` and `scripts/setup-worktree-debug-port.sh` |
| `test_sync_comments_grep.sh` | Tests sync_comments grep pattern correctness |
| `test_test_helpers.sh` | Tests the `test_helpers.sh` shared library |
| `test_tree_sitter_pipeline.sh` | Integration tests for `scripts/tree-sitter-generate.sh` |
