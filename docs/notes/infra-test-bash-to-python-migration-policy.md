# Infra Tests — the bash→Python Migration Policy

**Task #7430 | 2026-09-17**

---

## The rule

1. **A new infra test is authored in Python.** Not bash.
2. **An existing bash member is ported the next time it is touched to fix a
   flake.** The flake fix and the port land together.

There is **no scheduled mass migration**, and porting a member for its own sake
is not sanctioned work. `tests/infra` is 194 files and ~140k lines; a migration
run at that scale would itself be the largest source of gate churn on the
repo. The port cadence is deliberately coupled to the flake cadence so the
files that actually cost the gate move first and the quiet ones never move at
all.

A port is a **behaviour-preserving** rewrite of an existing member. It is not
licence to change what the test asserts. Because a port is green on arrival by
construction, RED-first proves nothing about it — establish non-vacuity by
**mutating the implementation** and confirming the port catches each mutant.
(Task 7430's port of `test_verify_env_ambient_isolation.sh` was validated this
way with five mutants; one was missed on the first pass and the assertion had
to be strengthened, which is the entire argument for doing this.)

---

## Why — the evidence

All figures measured 2026-09-17 in worktree `_lane-14` on branch `task/7430`,
base `main=8054d902d514`. Each is reproducible with the command given.

| Measurement | Value | How |
|---|---|---|
| Bash infra tests | **194 files, 139,665 lines** | `ls tests/infra/*.sh \| wc -l`; `cat tests/infra/*.sh \| wc -l` |
| Wall-clock sites | **1,111 lines** matching `\b(sleep\|timeout)\b` | `grep -cE '\b(sleep\|timeout)\b' tests/infra/*.sh` |
| …their concentration | **689 of 1,111 (62%) live in 15 files** | same, `sort -t: -k2 -rn \| head -15` |
| Flaky-ledger members | **20 distinct, and 20/20 are `tests/infra/*.sh`** | `data/verify-logs/flaky-ledger.jsonl`, group by `test` |

The concentration number is the one that shapes the policy. The 15 heaviest
files are heavy because their *subject* is wall-clock scheduling —
`test_portable_timeout.sh` (110), `test_proc_reaper.sh` (67),
`test_cpu_admit.sh` (64), `test_jobserver_balancer.sh` (52). Those are not
accidental sleeps to be refactored away; they are the thing under test. Porting
them buys little. The flake cost is instead spread across the *other* ~180
files, where subprocess orchestration, capture and timing are incidental to
what is being asserted and are exactly what Python's `subprocess` and
`unittest` handle without hand-rolled shell.

**100% of recorded flakes are bash infra members.** That is measured (20/20
above), and it is the single strongest argument here: the flaky ledger has
never recorded a non-`tests/infra` member, and has never recorded a Python one.

One figure in task 7430's filing is **not** re-measured here and should not be
cited as if it were: *"46% of 30-day merge-gate failures"*. It is plausible and
consistent with the above, but the per-task logs under
`data/verify-logs/<task>/` were not parsed for this note. Treat it as the
filing's claim, not as a measurement.

---

## The top-5 ledger members, and who owns each

Ranking measured 2026-09-17 against `data/verify-logs/flaky-ledger.jsonl`
(168 records, 155 distinct `run_id`, 20 distinct members) via
`python3 scripts/flake-density-report.py --top 5`:

| # | Member | Flakes | Disposition |
|---|---|---|---|
| 1 | `test_plan_capture_lib.sh` | 24 | **Fixed by #7430** |
| 2 | `test_occt_flock_gate.sh` | 22 | **Fixed by #7430** |
| 3 | `test_verify_env_ambient_isolation.sh` | 18 | **Fixed by #7430** |
| 4 | `test_run_all_ambient_isolation.sh` | 14 | Deferred → **#7622** |
| 5 | `test_seed_warm_lane.sh` | 11 | Deferred → **#7622** |

Ranks #1–#3 are 64 of the 168 recorded flakes and share **one** root cause, in
two shared libraries rather than in any of the three files:

- `tests/infra/plan_capture_lib.sh` — `plan_capture_complete()` certified a
  capture non-truncated from two header markers that land on lines 1 and 11 of
  a 39-line plan, so every truncation after line 11 (the entire command body,
  the only region the assertions read) was certified complete; and
  `capture_print_plan()` discarded the child's exit status, so a producer that
  emitted the markers and then died was accepted.
- `tests/infra/occt_flock_gate_lib.sh` — `occt_plan_grep_or_dump()` matched via
  `printf | grep -qE` under `set -o pipefail`, which returns 141 for a
  *present* pattern once the plan exceeds the 64 KiB pipe buffer; and it dumped
  only the child's stderr on no-match, which was empty in 25/25 healthy runs,
  so a failure archived no information about the plan that failed to match.

Rank #3 is a pure amplifier of #2: it runs the real `test_occt_flock_gate.sh`
as a nested subprocess, so any assertion that flakes inside #2 reds #3 too.

**Note the shape of that fix, because it is the policy's own counter-example:**
the defect was in shared bash libraries and was fixed *in bash*. Porting any of
the three files would not have fixed it. Ranks #4 and #5 do not share this root
cause and were not investigated — #7622 owns root-causing them, and each fix
there is also its port trigger under rule 2 above.

---

## Mechanics

### stdlib `unittest`, never pytest

pytest resolves on an interactive shell **only** because dark-factory's
virtualenv is on `PATH`. The interpreter the gate actually reaches,
`/usr/bin/python3` (3.12.3), raises `ModuleNotFoundError` for pytest, and the
repo has no `pyproject.toml`, `setup.cfg`, `pytest.ini`, `tox.ini`,
`conftest.py` or requirements file anywhere to anchor one.

A pytest-based infra test would therefore pass for its author and fail — or
silently not run — in the gate. That is precisely the green-standalone /
red-in-pipeline split that the ambient-isolation guards exist to catch, so
introducing it *inside the test suite that catches it* is not a trade worth
making. Add no third-party dependency.

The house idiom is `sys.path.insert` for co-located imports plus a closing
`unittest.main()`, as in `scripts/test_sn_gate.py`.

### A Python member is invisible until it has a `.sh` wrapper

`run_all.sh` discovers `test_*.sh` and nothing else. A bare `test_<name>.py`
under `tests/infra/` matches no glob, takes no classification-manifest row, and
is never executed — **it reads as coverage while asserting nothing.**

The fix is a ~30-line `tests/infra/test_<name>.sh` wrapper that asserts
`command -v python3` and that `python3 <the .py>` exits 0. The **wrapper** is
what `run_all.sh` discovers and what takes the
`run-all-classification.manifest` row. Four existing wrappers to copy:
`test_sn_gate.sh`, `test_prd_capability_check.sh`,
`test_prd_decompose_verify.sh`, `test_reify_overlap_detector.sh`.

This trap is not hypothetical. **`scripts/test_legibility_reify_config.py` has
no wrapper and no runner of any kind** — the only reference to it in the tree
is a prose comment in `docs/legibility/legibility.yaml` — and it is **red
today** (11 tests, 4 failures, measured 2026-09-17). It rotted unobserved for
exactly as long as it has existed. Task 7430's own plan mis-listed it as a
working precedent, which is how it stayed invisible through a review. Filed as
a follow-up.

### `run_all.sh` discovery is deliberately unchanged — and what that costs

Teaching discovery to glob `test_*.py` was considered and rejected **for task
7430 only**: it means editing a predicate duplicated at `run_all.sh:1347`,
`:1456` and `:2013` plus `classification_discovered_set`
(`run-all-classification-lib.sh:177`), changing the manifest grammar, and
re-satisfying four bidirectional asserts in `test_run_all_classification.sh` —
a large blast radius on load-bearing verify-pipeline artifacts, inside a task
whose purpose was *reducing* infra-gate reds.

That deferral has three measured costs. All are real; none was hidden:

1. **Every Python member needs a hand-written wrapper.** Forgetting the
   manifest row is caught (the classification gate fails in both directions);
   forgetting the *wrapper* is caught by nothing.
2. **The wall-clock upper-bound ratchet does not see Python.**
   `test_no_new_wallclock_upper_bounds.sh` scans `"$dir"/*.sh` (`:107`), so a
   wall-clock upper-bound assert written in Python is outside its scan scope. A
   port therefore moves code out from under an active ratchet.
3. **The deadline-capable-suite derivation does not see Python.**
   `test_slot_timeout_marker.sh`'s Section F builds its closure from
   `test_*.sh` + `run_all.sh` (`_f_node_list`, `:1704`) and admits only the
   verbs `bash|sh|source` when matching an invocation edge (`F_EDGE_VERB_RE`,
   `:1743`). A `python3`-verb invocation creates no edge, so a ported member
   silently drops out of the derived roster. Measured: replacing
   `test_verify_env_ambient_isolation.sh` with a wrapper takes that guard from
   144/0 to 141/3 (`FC6b`, `F1`, `G3`).

Costs 1 and 2 are **task #7445**'s charter (native `test_*.py` discovery, which
retires the wrapper idiom, plus widening the wall-clock guard's scan scope).
Cost 3 is not yet owned by any task and is the one to watch: it is a hole the
migration policy itself opens, and it widens with every future port.

---

## Reading the ledger: the denominator caveat

`data/verify-logs/flaky-ledger.jsonl` is append-only, and **a run with zero
flaky members writes no line at all** (`run_all.sh:728`). The file therefore
describes only the runs that already flaked.

Consequences, all of which `scripts/flake-density-report.py` is built to
respect:

- **Rows are not runs.** 168 records span 155 distinct `run_id`.
- **A true flakes-per-gate density is not derivable from this file.** There is
  no total-run count in it. Dividing flakes by rows or by distinct `run_id`
  yields a number near 1.0 regardless of fleet health. The report emits a
  density only when given an external `--total-runs N`, and otherwise says so.
- **`task` and `branch` carry no signal.** They are `"unknown"` and `"HEAD"` in
  every record, because the merge-verify lane runs detached and `run_all.sh`'s
  `task/*` case never matches. Grouping by either produces one bucket. Group by
  `test`, and secondarily by `role` (merge vs background).

This JSONL is also the **older of two ledgers**: dark-factory's
`plans/flake-ledger-prd.md` intends to supersede it with a SQLite ledger in
`runs.db`. Do not conflate counts across the two.
