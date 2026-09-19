# Infra Tests — the bash→Python Migration Policy

**Task #7430 | 2026-09-17**

---

## The rule

The policy has two arms. **Both are now in force** (Arm 2 reopened by task
#7626, 2026-09-19).

### Arm 1 — new infra tests are authored in Python. **ACTIVE.**

Not bash. This arm is demonstrated end to end and carries no open
prerequisite: `tests/infra/test_flake_density_report.py` and its thin
`test_flake_density_report.sh` wrapper landed green together, and the wrapper
holds `run-all-classification.manifest` row `:99`, so `run_all.sh` executes a
Python member for real today. Copy that pair when adding a new infra test.

### Arm 2 — port a bash member when it is next touched for a flake. **ACTIVE.**

A flake fix to a bash member and that member's port to Python land together,
with **no scheduled mass migration** — `tests/infra` is 194 files and ~140k
lines, and a migration run at that scale would itself be the largest source of
gate churn on the repo. Coupling the port cadence to the flake cadence moves
the files that actually cost the gate and leaves the quiet ones alone.

This arm was deferred until **task #7626**, because porting a member used to
remove it *silently* from a load-bearing guard. `test_slot_timeout_marker.sh`
derives which suites are deadline-capable, and whether each such site leaks
stderr, by reading the **text of sibling `.sh` files**, and its
invocation-edge grammar admitted only the verbs `bash`, `sh` and `source`.
#7626 taught Section F a **text-attribution** rule and a Python edge dialect,
and Section G a Python diversion dialect, so a ported member now **stays** in
F's derived roster with its real route and **stays** inside G's non-vacuity
check. A `.py` is still never a node: the roster, `D_ROSTER`, the `G0` slice,
the manifest row and every doc reference stay keyed on the `.sh` basename.

`test_verify_env_ambient_isolation` is the landed proof, and it lives in the
tree now rather than in a branch-local commit —
`tests/infra/test_verify_env_ambient_isolation.py` (540 lines, 26/26) behind
the thin `tests/infra/test_verify_env_ambient_isolation.sh` wrapper. `FC6b`
pins its derived route as `via:test_occt_flock_gate.sh` — *not*
`via:run_all.sh`, which is what a grammar loose enough to read docstring prose
as an invocation would have derived — and `G3`/`G1` read the spawn at
`test_verify_env_ambient_isolation.py:175-178` for 1 site / 0 unredirected.

### The Python dialect a port must emit

Sections F and G read a port's **text**, so the shapes below are a
**contract**, not a free choice. A port that spells its invocation another way
runs correctly and still drops out of one or both guards.

1. **The wrapper must really run the sibling.** Attribution is gated on an
   anchored `python3`/`python` invocation of a same-stem `.py`, never on the
   sibling merely existing — a stray unrun `.py` stays inert here for the same
   reason `run_all.sh`'s `test_*.sh` glob makes it inert there.
2. **The nested invocation must be an argv list headed by a QUOTED exec
   verb** — `["bash", str(NESTED)]`. The target is either bound by a real
   assignment (`NESTED = SCRIPT_DIR / "test_x.sh"`, `str()` and `os.fspath()`
   both fine) or written as a string literal inside that same list. A list
   headed by a bare name (`[sys.executable, ...]`) is deliberately not an
   invocation for this purpose, and neither is a **docstring** naming the
   suite in prose — `#`-stripping is shared with Python but does not remove a
   docstring, so prose is rejected by rule rather than by luck.
3. **The spawn must divert stderr** — `stderr=subprocess.PIPE`,
   `stderr=subprocess.DEVNULL`, `capture_output=True`, or
   `stderr=subprocess.STDOUT` **paired with a diverted stdout**. Unlike bash,
   the merge branch takes no order test: Python kwargs carry no ordering, so
   `stdout=..., stderr=...` and the reverse are the same call.
4. **Write a multi-line spawn with its closing paren on its own line**, or
   keep the whole call on one line. Section G stamps a multi-line spawn from
   its body at a line beginning `)`; a call whose closing paren trails the
   last kwarg is never seen to close, and the fail-safe counts an unclosed
   spawn as NOT captured. That direction is deliberate — it costs a false RED,
   never a false green — but it is still a RED you do not want.

### What a port is

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

That deferral has four measured costs. All are real; none is hidden — though
cost 4 below *was*, and how it stayed off this list is part of its entry:

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
3. **The deadline-capable-suite derivation did not see Python. CLOSED by
   #7626.** This was the cost that deferred Arm 2 outright. It was not fixed
   by widening the node set — a `.py` is still not a node — but by giving
   `_f_strip_node` a text-attribution rule that reads a delegating wrapper
   together with its sibling, plus a separately-named Python edge dialect and,
   in Section G, a Python diversion dialect selected from the scan target's
   extension. See "The Python dialect a port must emit" above for the shapes a
   port must now write.
4. **The merge gate's full-gate admission oracle did not see Python.
   IDENTIFIED AND CLOSED by #7626 — and it had never been on this list.**
   Measured before that task: this document contained zero matches for
   *verify-pipeline-guard*, *full gate*, *trivial-pass* or *config-only*. That
   omission is exactly why #7626 could close cost 3 and still ship this one
   open. It is recorded here rather than quietly patched, because a cost list
   that merely *looks* complete is how the next gap gets missed the same way.

   `scripts/verify-pipeline-guard.sh` is the oracle dark-factory's merge worker
   consults to decide whether a diff may take the trivial-pass fast path. Its
   infra-test clause matched `^tests/infra/[^/]*\.sh$` — so an Arm 2 port split
   a single member across that boundary: the thin wrapper kept routing to the
   full gate while the sibling holding every assertion classified as
   config-only, and the *identical* assertion edit changed route purely by
   having moved file. Measured on the port itself before the fix:

   | path | `requires-full-gate` |
   |---|---|
   | `tests/infra/test_verify_env_ambient_isolation.sh` | exit 0 |
   | `tests/infra/test_verify_env_ambient_isolation.py` | exit 1 |
   | `tests/infra/test_flake_density_report.py` | exit 1 |
   | `tests/infra/cpu_gov_instrument.py` | exit 1 |

   with no `.py` row anywhere in `scripts/verify-pipeline-paths.txt` to catch
   them by another route. This is a regression **Arm 2 creates**: Arm 1 only
   ever produced new tests, wrapper and sibling both new together; Arm 2 moves
   an *existing* gate member's body across the clause.

   Closed by widening that clause to `^tests/infra/[^/]*\.(sh|py)$`, lifted to
   one shared `_INFRA_GLOB_ERE` constant so the `requires-full-gate` and
   `is-registered` arms cannot drift apart. The rule stays directory-anchored
   and is deliberately NOT `test_`-prefixed, so a port may add a plain Python
   helper beside its member without a second gate edit —
   `tests/infra/cpu_gov_instrument.py`, driven directly by
   `test_cpu_load_governance.sh`, is the live case that forced that shape.

   **The check a future porter runs**, on every new file under `tests/infra/`:

   ```
   bash scripts/verify-pipeline-guard.sh requires-full-gate tests/infra/<new-file>
   ```

   Exit 0 means the gate sees it. Exit 1 on a file that holds assertions means
   the gate does not, and editing those assertions will fast-path past them.

Costs 1 and 2 are **task #7445**'s charter (native `test_*.py` discovery, which
retires the wrapper idiom, plus teaching the wall-clock guard Python — read
its Part B as the grammar extension cost 2 describes, not only the scan-scope
widening it is worded as). Costs 3 and 4 are closed.

Costs 2, 3 and 4 are one family, and naming it is the cheapest defence against
a fifth: each is a **guard whose scan scope or grammar is bash-shaped**, so
moving a member's body into `.py` moves it out from under that guard while
every roster, row and wrapper still looks intact. Cost 1 is the odd one out —
it is about discovery, not about a guard going quiet. When auditing for the
next gap, enumerate the guards, not the rosters.

They fail in different directions, which is what decides how each is held.
Cost 1 fails **loudly**: a missing manifest row reds the classification gate in
both directions. Cost 2 fails **quietly but non-silently** — an unratcheted
wall-clock assert still runs and still asserts, so no coverage is lost; what is
lost is the ratchet that stops a retired flake class from being re-admitted,
which is why cost 2 is carried by the interim rule above rather than by a
guard. Cost 3 **was** the one to watch, and that analysis is kept because it
is what decided the sequencing: it failed **silently and in the green
direction** — the member keeps passing, the roster keeps deriving, and a
deadline-capability guard simply stops covering one suite. A migration policy
that traded loud coverage for quiet coverage loss would be worse than no
policy, which is why Arm 2 waited for #7626 rather than shipping alongside it.
Cost 4 failed the same way but one level worse, and that is why it is worth
the space it takes above: cost 3 lost coverage *within* a gate that still ran,
whereas cost 4 skipped the gate entirely — the diff is classified config-only,
takes the merge worker's trivial pass, and lands on `main` without the ported
member ever having been executed. Nothing reds, so the only signal is the
oracle, which is why the porter's check above is a step and not a suggestion.

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
