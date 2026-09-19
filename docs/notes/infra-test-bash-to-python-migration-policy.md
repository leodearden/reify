# Infra Tests — the bash→Python Migration Policy

**Task #7430 | 2026-09-17**

---

## The rule

The policy has two arms. **They are not both in force yet.**

### Arm 1 — new infra tests are authored in Python. **ACTIVE.**

Not bash. This arm is demonstrated end to end and carries no open
prerequisite: `tests/infra/test_flake_density_report.py` and its thin
`test_flake_density_report.sh` wrapper landed green together, and the wrapper
holds `run-all-classification.manifest` row `:99`, so `run_all.sh` executes a
Python member for real today. Copy that pair when adding a new infra test.

### Arm 2 — port a bash member when it is next touched for a flake. **DEFERRED.**

The intended rule is that a flake fix to a bash member and that member's port
to Python land together, with **no scheduled mass migration** — `tests/infra`
is 194 files and ~140k lines, and a migration run at that scale would itself be
the largest source of gate churn on the repo. Coupling the port cadence to the
flake cadence moves the files that actually cost the gate and leaves the quiet
ones alone.

**Do not port a member today.** Porting one silently removes it from a
load-bearing guard. `test_slot_timeout_marker.sh` derives which suites are
deadline-capable, and whether each such site leaks stderr, by reading the
**text of sibling `.sh` files**: its Section F builds the closure from
`test_*.sh` plus `run_all.sh`, and its invocation-edge grammar admits only the
verbs `bash`, `sh` and `source`. A ported member is therefore not a node, and a
`python3`-verb invocation of a nested suite creates no edge — so the member
drops out of F's derived roster and out of Section G's non-vacuity check
**without any assertion failing to announce it**. Measured on the one port
attempted: replacing `test_verify_env_ambient_isolation.sh` with its wrapper
took that guard from 144/0 to 141/3 (`FC6b`, `F1`, `G3`).

Teaching Sections F and G a Python-sibling shape is the prerequisite, and it is
not a small edit: every rule in that grammar carries a measured
false-admission rationale, and widening it carelessly produces **false greens
in a deadline-capability check** — the opposite of what this policy is for. It
needs its own RED and its own review. Owned by **task #7626** (filed as ticket
`tkt_0RTQTESPHBBB84KKAJCHKJF6X0`); once it lands, this arm reopens.

That follow-up also carries the port already written for
`test_verify_env_ambient_isolation.sh`: 540 lines, 26/26 green, validated by
five mutants rather than RED-first. **Recover it from git history rather than
rewriting it** — find it by **commit subject**, *"Port the verify_env
ambient-isolation guard to Python"*, via
`git log --all --oneline --grep='Port the verify_env'`.

Match on the subject, not on a hash. Any SHA for it is branch-local and
therefore unstable: it was `750fc72439` when task 7430's escalation quoted it,
`eac9251c59` after that lane was rebased, and it becomes something else again
if the branch is rebased before merge. A SHA cited in a committed doc is only
durable when it names a **main** commit; this one never can, because the
commit's whole purpose is to hold a file that main does not keep.

### What a port is, when the arm reopens

A port is a **behaviour-preserving** rewrite. It is not licence to change what
the test asserts. Because a port is green on arrival by construction, RED-first
proves nothing about it — establish non-vacuity by **mutating the
implementation** and confirming the port catches each mutant. In the one port
done so far, one of five mutants was *missed* on the first pass and the
assertion had to be strengthened. That is the entire argument for doing this.

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
cause and were not investigated — #7622 owns root-causing them. Under Arm 2
each of those fixes would also be its port trigger; while Arm 2 is deferred,
#7622 fixes them in bash and the ports wait with everyone else's.

This is also why deferring Arm 2 costs less than it appears: **not one of the
five was fixed by porting it.** Ranks #1–#3 were closed by a library fix in
bash, and #4/#5 are bash fixes too. The migration is a maintainability policy,
not the flake remedy.

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
`run-all-classification.manifest` row. Wrappers to copy — `test_sn_gate.sh`,
`test_prd_capability_check.sh`, `test_prd_decompose_verify.sh`,
`test_reify_overlap_detector.sh`, and `test_flake_density_report.sh`, the one
this policy landed and the only one whose `.py` lives beside it in
`tests/infra/` rather than in `scripts/`.

This trap is not hypothetical. **`scripts/test_legibility_reify_config.py` has
no wrapper and no runner of any kind** — the only reference to it in the tree
is a prose comment in `docs/legibility/legibility.yaml` — and it is **red
today** (11 tests, 4 failures, measured 2026-09-17). It rotted unobserved for
exactly as long as it has existed. Task 7430's own plan mis-listed it as a
working precedent, which is how it stayed invisible through a review — the
lesson being that a plan's *enumerated lists* need re-deriving just as much as
its numbers. Owned by **task #7627** (filed as ticket
`tkt_0RTQTRYZHDGW40K1C73PAZJV6N`), which records that it must be fixed before
it is wrapped: wrapping it while red would land a red gate member.

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
2. **The wall-clock upper-bound ratchet does not see Python — and widening its
   glob would not fix that.** `test_no_new_wallclock_upper_bounds.sh` is a
   NEW-construct ratchet: it flags new absolute-wall-clock upper-bound asserts
   so the flake class tasks 4841-4847 retired cannot silently return. It scans
   `"$dir"/*.sh` (`:107`), so a Python member is out of scan scope — but its
   detector is **bash-grammar-bound** as well, so scan scope is only half the
   gap. A violation must satisfy three conditions on one logical line
   (`:223-228`), and the upper-bound one is `_op_re='-l[et][[:space:]][0-9]'`
   (`:78`) — a `test`-builtin operator. Measured against
   `self.assertLess(elapsed, 5.0, "boot under 5s")`: the assert-wired and
   time-lexeme conditions both match, the operator condition does not. The fix
   is a **grammar extension** — a Python operator dialect — not a glob
   widening; #7445 Part B is currently written as the latter.

   **Interim rule, until #7445 lands: a new Python infra member must not assert
   a wall-clock upper bound at all.** Arm 1 is ACTIVE and routes every new infra
   test to Python, so this is the cost that bites TODAY rather than at the next
   port: the door the ratchet exists to hold shut stands open for exactly the
   population the policy now sends through it. A bound you cannot avoid belongs
   in a `.sh` member until the grammar can see it.
3. **The deadline-capable-suite derivation does not see Python.** Entry points
   are `_f_node_list` (`test_slot_timeout_marker.sh:1704`) and
   `F_EDGE_VERB_RE` (`:1743`). This is the cost that **defers Arm 2 outright**
   — see "The rule" above for the mechanism and the measurement; it is not
   restated here.

Costs 1 and 2 are **task #7445**'s charter (native `test_*.py` discovery, which
retires the wrapper idiom, plus teaching the wall-clock guard Python — read
its Part B as the grammar extension cost 2 describes, not only the scan-scope
widening it is worded as). Cost 3 is owned by **task #7626**, described under
Arm 2.

The three fail in different directions, which is what decides how each is held.
Cost 1 fails **loudly**: a missing manifest row reds the classification gate in
both directions. Cost 2 fails **quietly but non-silently** — an unratcheted
wall-clock assert still runs and still asserts, so no coverage is lost; what is
lost is the ratchet that stops a retired flake class from being re-admitted,
which is why cost 2 is carried by the interim rule above rather than by a
guard. Cost 3 is the one to watch: it fails **silently and in the green
direction** — the member keeps passing, the roster keeps deriving, and a
deadline-capability guard simply stops covering one suite. A migration policy
that traded loud coverage for quiet coverage loss would be worse than no
policy, which is why Arm 2 waits.

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
