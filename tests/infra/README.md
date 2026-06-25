# tests/infra/

Shell meta-tests for reify's infrastructure scripts (`scripts/lib_portable.sh`,
`scripts/tree-sitter-generate.sh`, `scripts/test_pm_standardization.sh`, etc.).

## Auto-discovery

`run_all.sh` discovers and runs every file matching `test_*.sh` in this directory.
To add a new meta-test, create `test_<name>.sh` — it will be picked up automatically
on the next `run_all.sh` invocation and in CI.

**Exception:** `test_helpers.sh` is a shared library, not a test runner.
It is excluded from discovery by exact name.

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

`run_all.sh` is wired into `orchestrator.yaml`'s `test_command` via:

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
(`-le` / `-lt`), (3) wall-clock lexeme in description or variable name
(`elapsed`, `within Ns`, `ELAPSED`, `_MS`, `_NS`, `SECONDS`, etc.),
(4) no inline `wallclock:allow` escape.

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

**Current blessed survivors** (as of task #4848):
- `test_occt_flock_gate.sh` Tests 14 & 22: exit-75 + stderr pattern
- `test_find_uses_smoke_runner.sh` liveness guard: rc!=0 + launcher-death message

## Files

| File | Purpose |
|------|---------|
| `run_all.sh` | Discovery runner — runs all `test_*.sh` files |
| `test_helpers.sh` | Shared library: `assert()` and `test_summary()` |
| `test_no_new_wallclock_upper_bounds.sh` | Regression guard: static-grep for new wall-clock upper-bound asserts |
| `test_npm_ci_hardening.sh` | Tests npm ci guard conventions in orchestrator.yaml |
| `test_portable_sha256.sh` | Tests `portable_sha256()` from `scripts/lib_portable.sh` |
| `test_portable_timeout.sh` | Tests `portable_timeout()` from `scripts/lib_portable.sh` |
| `test_release_mode_in_test_command.sh` | Tests orchestrator.yaml runs cargo test --release for release-only tests |
| `test_run_all.sh` | Tests this `run_all.sh` discovery runner |
| `test_setup_worktree_debug_port.sh` | Tests `allocate_free_port()` and `scripts/setup-worktree-debug-port.sh` |
| `test_sync_comments_grep.sh` | Tests sync_comments grep pattern correctness |
| `test_test_helpers.sh` | Tests the `test_helpers.sh` shared library |
| `test_tree_sitter_pipeline.sh` | Integration tests for `scripts/tree-sitter-generate.sh` |
