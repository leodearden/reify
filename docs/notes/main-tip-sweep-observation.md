# Main-tip sweep observation (runbook)

How to tell, from reify's side, whether dark-factory's main-tip integrity sweep
ran and what it concluded. Commands and measured facts only. Why the ledger is
built this way lives in `scripts/verify.sh`: the `REIFY_BACKGROUND_SWEEP_LEDGER`
entry in the header env block and the "Background-sweep verdict ledger" block.

## 1. Primary channel: the reify verdict ledger

`<main checkout>/data/orchestrator/verify-background-sweeps.jsonl` (gitignored by
`data/orchestrator/`). verify.sh appends one JSON line per COMPLETED
`DF_VERIFY_ROLE=background` run. The ledger is live once task 7423 lands.
`REIFY_BACKGROUND_SWEEP_LEDGER` overrides the path.

Locate the main checkout from any lane:

```bash
source scripts/lib_main_checkout.sh; reify_main_checkout
```

Record schema, exactly as `_record_background_verdict` (scripts/verify.sh) writes it:

| field | value |
|---|---|
| `role` | always `background` |
| `action` | the verify.sh action (`test`, `lint`, …) |
| `profiles` | the resolved `--profile` (`debug` / `release` / `both`) |
| `scope` | the resolved `--scope`; background forces `all` (contract C2) |
| `verdict` | `pass` (exit 0) or `fail` |
| `exit_code` | verify.sh's exit status |
| `head` | `git rev-parse HEAD` in the sweep worktree, i.e. the swept main SHA |
| `tree_oid` | `git rev-parse HEAD:` |
| `timestamp` | UTC, `%Y-%m-%dT%H:%M:%SZ` |

Read the recent records:

```bash
tail -n 20 /home/leo/src/reify/data/orchestrator/verify-background-sweeps.jsonl \
  | jq -c '{timestamp,head:.head[0:10],action,verdict,exit_code}'
```

Read one sweep: group the records by `head`.

```bash
jq -s -c 'group_by(.head)[] | {head: .[0].head[0:10], runs: map({timestamp,action,verdict})}' \
  /home/leo/src/reify/data/orchestrator/verify-background-sweeps.jsonl
```

What one sweep writes. Observed by reading dark-factory main `d8ea20e0e8` (read-only):

- `run_main_tip_sweep` calls `run_full_verification(..., role='background')` once per pass.
- reify declares no per-subproject orchestrator config, so that call is one global
  `run_verification`. It runs reify's `test_command` and `lint_command`, both
  `./scripts/verify.sh … --scope branch --include-infra`, plus `type_check_command`,
  which is `true` and so writes no record.
- **One pass therefore appends TWO records for its head: `action=test` and `action=lint`.**
- A failed first pass triggers ONE full retry in the same worktree. The retry
  appends a second test+lint pair for the same head. Example: e10e72d7 failed
  its first pass and passed its retry.
- The isolated pre-filter (`_sweep_failure_reproduces_in_isolation`) maps pytest
  node-ids onto subprojects. reify has none, so the pre-filter returns
  "unconfirmable" and adds no record.
- A pure-timeout leg re-runs the whole attempt under `verify_timeout_retries`,
  which appends another pair.

## 2. Secondary / pre-ledger channel: the dark-factory journal

```bash
journalctl --user -u orchestrator-reify.service --since '<start>' --until '<end>' \
  --no-pager -g 'run_main_tip_sweep|Main-tip integrity'
```

- `orchestrator-reify.service` is a systemd **user** unit. Measured 2026-10-07:
  `systemctl --user is-active` → `active`; `systemctl is-active` → `inactive`. The
  same window queried in system scope (no `--user`) returns `-- No entries --`.
- **Always bound the window.** Measured 2026-10-07:
  - A ±10-minute window around a known timestamp answers in 0.6–7 s.
  - `--since -6h` takes 11 s, and `--since -72h` takes 125 s.
  - `--since 2026-09-20` (about 17 days) did not finish within 600 s.

Line shapes (grep the text, not the level):

| outcome | logger | line |
|---|---|---|
| first pass failed, retrying | `orchestrator.verify` | `run_main_tip_sweep: first-pass verification failed at <sha12> (category=…) — retrying once …` |
| retry passed: flake suppressed, nothing filed | `orchestrator.verify` | `run_main_tip_sweep: first-pass failure at <sha12> did NOT reproduce on retry …` |
| retry failed: drift handed to the harness | `orchestrator.verify` | `run_main_tip_sweep: first-pass failure at <sha12> REPRODUCED on retry …` (in DF source; not yet seen in the journal) |
| escalation filed | `orchestrator.harness` | `Main-tip integrity sweep: filed L1 escalation esc-main-sweep-<sha12>-N for SHA <sha12> (<category>)` |
| confirmed, but the tip moved | `orchestrator.harness` (INFO) | `Main-tip integrity sweep: failure at <sha12> unconfirmed on current tip (subset_confirmed=… tip_unchanged=…) — not filing (stale/transient)` |

## 3. Caveats

a. A sweep that PASSES its first pass is journal-SILENT by construction until
   DF #5455 lands, and so is every non-failure early return. Absence of journal
   lines is NOT evidence that no sweep ran.
b. A minted `_mainsweep-<hex>` worktree with NO `target/` and a HELD
   `<name>.lock` is a sweep QUEUED for the shared verify admission slot, not a
   vacuous one. Probe: `flock -n -s <dir>.lock true` failing means the lock is held.
   Leaked trees are restart-interrupted sweeps; reclaiming them is DF #5584.
c. One sweep pass may legitimately run about 2 h (e10e72d7's retry ran 1h56m).
   Do not call a hang before then.
d. The ledger FAILS OPEN by design: an unresolvable checkout or an unwritable path
   means no record and an untouched exit code. Cross-check a missing record
   against the journal before concluding anything.
e. The esc-7423-7/-8 "~50% vacuous sweeps" reading was a biased sample (Leo,
   2026-09-23). It is recorded here so nobody re-derives it.
f. The two records already in the ledger are NOT sweeps. Both are dated
   2026-09-21 at `ec35141500` (14:43:35Z and 14:46:20Z). `ec35141500` is
   the speculative "Merge task/7423 into main" commit of that day's 7423
   merge-verify and is on no branch. Their shape (`action=test`,
   `scope=all`, `profiles=both`, `pass`) matches the one infra suite that
   ran the real `verify.sh` with `DF_VERIFY_ROLE=background` and no
   scratch `REIFY_BACKGROUND_SWEEP_LEDGER`:
   tests/infra/test_verify_semaphore_e2e.sh Section H, which runs both
   directly and nested via test_verify_nextest_absent_suites.sh. Since
   `a612418e0a` Section H points the knob at a scratch file and asserts
   the record lands there, so it adds no more.
   A sweep's `head` is always a main commit at sweep time, so
   `git -C /home/leo/src/reify merge-base --is-ancestor <head> main` failing
   rules a record out. The check is one-directional: a merge-verify
   commit that then lands is on main too.

## 4. Ownership

This note covers the reify-side check only. The sweep itself is dark-factory's:

- DF #5455: sweep lifecycle log lines and runs.db events (the upstream answer to caveat a).
- DF #5584: reclaim leaked `_mainsweep-*` trees.
- DF #5812: warm-seed the sweep.

All three were `pending` on 2026-10-07.

## 5. Observed (task 7423, 2026-10-07)

Produced by re-running the commands above. Each item says which channel it went
through, because only the journal is live on main today.

**Harness-driven sweep completions, through the journal (live on main).** The
§2 command, with the window shown and its measured elapsed time:

1. `--since '2026-09-21 12:40' --until '2026-09-21 13:00'` (5.4 s). e10e72d7's
   first pass failed and its retry passed (the retry ran 1h56m):
   ```
   Sep 21 12:51:34 leo-MS-7C35 uv[1237512]: 2026-09-21 12:51:34 WARNING  [orchestrator.verify] run_main_tip_sweep: first-pass failure at e10e72d7da41 did NOT reproduce on retry (first-pass category=<FailureCategory.TREE_SITTER_GENERATE_ERROR: 'tree_sitter_generate_error'>, cause_hint='error: test run failed') — treating as transient flake and suppressing drift escalation. NOTE: retry-on-flake MAY MASK a real intermittent regression introduced by a recent merge.
   ```
2. `--since '2026-09-22 12:20' --until '2026-09-22 12:40'` (1.6 s). A red main was
   caught and escalated:
   ```
   Sep 22 12:31:15 leo-MS-7C35 uv[1237512]: 2026-09-22 12:31:15 WARNING  [orchestrator.harness] Main-tip integrity sweep: filed L1 escalation esc-main-sweep-e49c9ee21884-1 for SHA e49c9ee21884 (test_failure)
   ```
3. `--since '2026-09-23 09:10' --until '2026-09-23 09:30'` (0.6 s). The failure was
   confirmed, but the tip had moved:
   ```
   Sep 23 09:21:57 leo-MS-7C35 uv[1237512]: 2026-09-23 09:21:57 INFO     [orchestrator.harness] Main-tip integrity sweep: failure at 1bfe7d9471a4 unconfirmed on current tip (subset_confirmed=True tip_unchanged=False) — not filing (stale/transient)
   ```
4. `--since -24h`, run at 2026-10-07T11:20:48Z (28.9 s): `-- No entries --`. No
   sweep failed in that window. Whether one passed is unreadable here (caveat a).

**Offline lane entry point (GAP 1), executed in this lane.**
`bash scripts/run-offline-deep.sh --test-threads=1 --confirm-failed; echo rc=$?`
on task/7423 `a612418e0a` with no confirm manifest under `target/` gave
`rc=0` and 0 bytes on merged stdout+stderr (0.18 s). Before 7423, this exact
argv exited 64 at verify.sh's argument parser.

**Ledger (§1).**
- *Recording path, executed in this branch:* tests/infra/test_verify_background_sweep_ledger.sh
  runs the real `verify.sh` with `DF_VERIFY_ROLE=background` against a scratch ledger.
- *Harness-driven record, deferred:* the main checkout's ledger holds only the two
  caveat-f records, at `ec35141500` (`merge-base --is-ancestor … main` → 1). No
  sweep has written to it yet. The first live record appears on the first sweep after
  7423 lands on main. Closing 7423 does not wait for it (Leo, 2026-09-23). To check
  later, re-run the §1 `tail` command and look for a `head` that passes the
  `--is-ancestor` check. Never write a synthetic record into the real ledger: tests use
  scratch `REIFY_BACKGROUND_SWEEP_LEDGER` paths only.
