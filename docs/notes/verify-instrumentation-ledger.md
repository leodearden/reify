# Verify instrumentation ledger

Task 7424 implements Leo's ruling **R13** (2026-09-11; verify-speed study
`plans/verify-speed-study-reify-2026-09-10.md`, item 1, §1.2, AL-5). Before
this task, no **passing** reify gate left any phase record. The study's phase
profile was therefore reconstructed from 39 *failed* logs, and that sample is
biased. Each verify run now appends one JSON record to an append-only ledger,
and lane seeding leaves a stamp that the record reads.

Consumers:

| Consumer | Reads |
|---|---|
| **E1**: phase walls and per-verify sccache for passing gates | `steps[].wall_secs`, `steps[].nextest`, `steps[].sccache`, `steps[].run_all` |
| **E2**: the seed→first-build tax in production (feeds **D6**) | `lane_seed` |
| **E11**: cold rebuild after the debuginfo profile change (8c/8d) | `debug_nextest` `nextest.pre_exec_secs`, split at a cutover date |
| **E12**: wrapper-off + incremental A/B (8b shape ii) | compile residue per arm; the arm shows in `sccache.before.reason` |
| **E13**: release-skip firing rate | `release_pass.decision` |
| **D6**: fund the `env!()` relocation sweep only if the tax is ≥ 1 time-averaged core | E2's numbers |

## Where the ledger lives, and why reify owns it

`<main checkout>/data/verify-logs/verify-runs.jsonl`. `REIFY_VERIFY_LEDGER_DIR`
overrides the directory. Writers are serialised by `flock` on the sibling
`verify-runs.jsonl.lock`, and each record is a single `O_APPEND` write of one
line. `data/verify-logs/` is git-ignored. It also holds the flaky ledger and
`run-all-skip-state.json`.

Reify owns the ledger because dark-factory archives nothing from a passing
verify that reify could extend. It lives in the **main** checkout, not the
running worktree, because every verify worktree is cleaned or deleted
afterwards (the `_merge-verify` reseed, the `_mainsweep-*` teardown). The main
checkout is resolved by `scripts/lib_main_checkout.sh`, the same resolution
`REIFY_BACKGROUND_SWEEP_LEDGER` uses.

## Activation

A run records only when **all** of the following hold:

- it is an executed plan; `--print-plan` never records;
- it is **top-level**: `REIFY_VERIFY_RUN_ID` is unset on entry;
- `python3` and `scripts/verify-ledger.py` are present;
- a ledger directory resolves: `REIFY_VERIFY_LEDGER_DIR`, or the default,
  which refuses a run beneath reify's infra harness (below).

A recording run exports `REIFY_VERIFY_RUN_ID` to its whole process tree. Any
verify.sh started beneath it inherits the id, counts as **nested**, and
records nothing. That covers the infra suites the gate runs.

A suite run by hand (`bash tests/infra/test_x.sh`, or
`bash tests/infra/run_all.sh`) has no recording run above it. Many suites
execute verify.sh for real with stub cargo, and their runs carry whatever role
the suite stamped, so the **default** ledger also refuses any run that has
reify's infra harness among its ancestors. The harness is a process whose
argv names `tests/infra/run_all.sh` or a `tests/infra/test_*.sh` / `.py`,
resolved against that process's cwd (`verify-ledger.py harness-ancestor`). A
suite that wants a record passes its own `REIFY_VERIFY_LEDGER_DIR`, as the
ledger's own suites do.

Every instrument is fail-open: it never changes the plan, stdout or the exit
code. A failure lands in the record's `instrument_errors`. A failure no record
can carry goes to verify.sh's stderr as a `verify-ledger.py:` line instead.
That covers a failed finalize: an unwritable ledger, a crash composing the
record, or a missing `meta.json` after a failed run-begin. verify.sh deletes
the journal right after finalize, so a failed finalize reports there along
with the journal errors its unwritten record would have carried. A verify
that a sandboxed agent starts by hand runs inside the agent's landlock
write-set. Where that set excludes the main checkout, the append fails and the
run leaves that stderr line instead of a record.

| Knob | Meaning |
|---|---|
| `REIFY_VERIFY_LEDGER_DIR` | Ledger directory (default `<main checkout>/data/verify-logs`, which refuses a run beneath the infra harness). |
| `REIFY_VERIFY_LEDGER_LANE_DIR` | Lane whose seed stamp the record reads (default: the running checkout). |
| `REIFY_VERIFY_RUN_ID` | Internal. Inheriting it marks a run as nested. |
| `REIFY_RUN_ALL_TIMING_OUT` | Internal. Exported around each plan step; `run_all.sh` consumes it, unsets it, and writes its timing there. |
| `REIFY_VERIFY_LEDGER_PROC_ROOT` | Test hook: the `/proc` the concurrency scan and the harness check read. |

While a run is in flight, its journal lives in
`${TMPDIR:-/tmp}/reify-verify-journal.XXXXXX`. Finalize runs once, from
verify.sh's EXIT/INT/TERM/HUP cleanup, composes the record and removes the
journal.

## Record schema `reify.verify-run/1`

One JSON object per line.

| Field | Meaning |
|---|---|
| `schema` | `"reify.verify-run/1"` |
| `run_id` | `<epoch>-<pid>-<random>`, the value exported as `REIFY_VERIFY_RUN_ID`. |
| `role`, `role_raw` | Effective `DF_VERIFY_ROLE` (defaults to `task`), and the raw inherited value (`null` if unset). |
| `action`, `profile`, `scope` | verify.sh's resolved arguments. |
| `lane` | The lane dir the seed stamp is read from. |
| `branch`, `head`, `tree_oid` | Git identity of the checkout that ran. |
| `started_at`, `ended_at`, `wall_secs` | Epoch seconds. `started_at` is taken at verify.sh's startup, `ended_at` in its exit cleanup. |
| `exit_code`, `verdict`, `signal` | The executor's exit (the failing step's rc; 130/143/129 on INT/TERM/HUP). `verdict` is `pass` or `fail`. `signal` is `INT`, `TERM`, `HUP` or `null`. |
| `release_pass` | `{decision, release_delta_skip_raw}`, described below. |
| `steps` | One object per plan entry, in plan order, executed or not. |
| `lane_seed` | Seed forensics, or `null` when the lane has no seed stamp. |
| `instrument_errors` | Lines describing instrument failures. Empty on a clean run. |

### `release_pass`

- `decision` is `skipped-delta-clean` when the plan carries the
  `RELEASE-PASS: skipped (delta-clean)` marker. It is `planned` when a release
  test phase is in the plan, and `absent` otherwise (for example a debug-only
  profile).
- `release_delta_skip_raw` is verify.sh's own `_RELEASE_DELTA_SKIP` decision
  (`"0"` or `"1"`), not the input knob.

### `steps[]`

| Field | Meaning |
|---|---|
| `index`, `cmd` | Position in the plan, and the plan line verbatim. |
| `phase` | Classifier output (below). |
| `began_at`, `ended_at`, `wall_secs`, `rc` | Per-step timing. All are `null` for a step that never ran, because the executor stops at the first failure. |
| `sccache` | `{before, after}`, raw `sccache --show-stats --stats-format=json` objects. Present on cargo-bearing steps only. When the wrapper is not sccache, or the call fails, the value is `{available: false, reason}` instead. |
| `concurrency` | `{before, after}`, one per sccache snapshot. Contents are described below. |
| `nextest` | The JUnit split, on nextest steps only. |
| `run_all` | `run_all.sh`'s timing, on any step whose process tree ran it (normally `infra_pool`). |

**Phases.** The normative definition is `Phase` / `PHASE_RULES` in
`scripts/verify-ledger.py`. The first matching rule wins.

- **Cargo-bearing** (these carry sccache and concurrency snapshots):
  `debug_nextest`, `release_nextest`, `gui_feature_nextest`,
  `debug_cargo_test`, `release_cargo_test`, `gui_feature_cargo_test`,
  `clippy`, `gui_feature_check`, `cargo_check`, `release_prebuild`,
  `cargo_other`.
- **Other phases:** `test_slot_acquire` and `test_slot_release` (the semaphore
  markers), `release_skip_marker`, `infra_pool`, `infra_selective`,
  `infra_static`, `psi_gate`, `compile_gate`, `gui_node_launch`,
  `gui_node_join`, `gui_node`, `postflight`, `bookkeeping`, `preflight`,
  `other`.

**Concurrency** fields:

- `other_verify_runs`: distinct outermost `verify.sh <action>` process trees
  other than this run.
- `cargo_in_other_verifies`: cargo processes inside those trees.
- `cargo_outside_verifies`: cargo processes outside any verify (agents and
  others).
- `rustc_total`: every rustc on the host, this run's included.
- `host`: `{loadavg_1, psi_cpu_some_avg10}`.

The scan reads `/proc`, so lanes still running an older verify.sh are
counted too.

**Nextest split.** A debug nextest step is a single build-plus-execute
command. It is split at the root `<testsuites timestamp time>` of the JUnit
report that `gen-nextest-config.sh` writes on every verify-driven nextest
pass. That report is `_CONFIRM_JUNIT_PATH`, read right after each step,
because the debug and release passes share the file. Fields:

- `exec_started_at`, `exec_secs`;
- `pre_exec_secs`: step begin → test execution start. This is **build plus
  nextest's test listing**, not compile time alone;
- `post_exec_secs`: execution end → step end;
- `tests`, `failures`, `errors`.

If no fresh report exists, the split is `{available: false, reason:
no-junit | stale-junit | unparseable-junit}`.

### `run_all` block

`{summary, members}`, written by `tests/infra/run_all.sh`.

`summary` is `{kind: "summary", path, run_id}`, where `path` is `pool`,
`host-infra`, `member-subset` or `legacy-serial`, and `run_id` is
run_all.sh's own `<epoch>-<pid>`. On the pool path the summary also has:

- `phases`: `{pool, serial, retry}`, each `{began_at, ended_at}`;
- `totals`: `{members, pool, serial, retried}`.

`members` lists one row per attempt:
`{kind: "member", name, phase: pool|serial|retry, queued_at, started_at,
ended_at, rc}`. `queued_at` is set on pool attempts only and marks the moment
before the slot wait. The serial-phase wall is
`phases.serial.ended_at − phases.serial.began_at`.

### `lane_seed` block and the `reify.lane-seed/1` stamp

**The stamp.** `scripts/seed-warm-lane.sh` writes
`<realpath(lane_dir)>.seed-stamp.json` atomically, as a **sibling** of the
lane next to `<lane>.lock`. It is written only at the end of a successful
seed: a refused or failed seed leaves any older stamp untouched, and a reseed
replaces it with a new `seed_id`. Fields:

- `schema`: `"reify.lane-seed/1"`;
- `seed_id`: 16 hex characters;
- `lane_dir`: the realpath;
- `mode`: `fresh-checkout` or `reset-in-place`;
- `started_at`: taken at the seed script's startup;
- `exited_at` (S): just before the stamp write.

Being a sibling, the stamp survives dark-factory's rmtree of `<lane>/target`,
stays outside `git status`, and is invisible to lane enumerators that glob
directories.

**The agent start log.** `scripts/agent-bin/cargo` appends
`{started_at, subcommand}` to `<lane>/target/.reify-cargo-starts.jsonl` for
heavy subcommands (`build test nextest check clippy bench doc build-std`)
when the cwd is inside a stamped lane. It appends after PSI admission and
before the `exec`. The log lives in `target/`, so a reseed discards it. The
shim is not on agents' PATH while agent CPU governance is off, so the log stays
empty today (see Consumer rules → Seed → first build).

**The block.** Finalize reads the lane but never writes, renames or deletes
anything in it.

- `seed`: the stamp verbatim.
- `cargo_starts`: every known cargo start after S, sorted. These are agent
  starts `{source: "agent", started_at, subcommand}` and this run's
  cargo-bearing step begins `{source: "verify", started_at, index, phase}`.
  Malformed agent lines become `instrument_errors`.
- `fingerprint`:
  `{units_total, units_written_since_seed, first_write_at, last_write_at}`.
  A unit is a `target/{debug,release}/.fingerprint/<unit>/` directory, and its
  write time is its newest **file** mtime: cargo rewrites fingerprint files in
  place, so the directory mtime does not move. Cloned units keep the base's
  older mtimes.
- `unattributed_writes`: `{units, first_write_at, last_write_at}` for units
  written after S but before the first known cargo start. Somebody built, and
  nothing observed who.
- `first_build`: `null` when no cargo start is known. Otherwise it has one of
  two shapes:
  - **Agent first** (`basis: "fingerprint-mtime"`). Fields: `source`,
    `started_at`, `window_end_at`, `ended_by`, `units_written`,
    `last_unit_write_at`, `seed_to_first_build_secs`, `basis`.
    `window_end_at` is the next known cargo start, or `null` with `ended_by:
    "no-later-cargo-start"`. `units_written` counts units whose newest mtime
    falls in `[started_at, window_end_at)`. `seed_to_first_build_secs =
    last_unit_write_at − S`.
  - **Verify first** (`basis: "verify-step-end"`). Fields: `source`,
    `started_at`, `index`, `phase`, `exited_at`, `rc`,
    `seed_to_first_build_secs`, `basis`. `seed_to_first_build_secs =
    exited_at − S`.

## Consumer rules

### sccache: ruling D10 (Leo, 2026-09-14, esc-7424-3)

- sccache numbers are **windows over a host-global counter**, never
  per-verify attribution. One daemon serves every lane, and its counters are
  cumulative since daemon start.
- **Bucket by concurrency.** A sample is one cargo step's before/after pair.
  Its bucket is the larger `other_verify_runs` of its two snapshots.
- **Report the zero-concurrency bucket first**, before drawing any conclusion
  from a hit rate.
- **A bucket with fewer than N samples is inconclusive.**
- Use the shipped report rather than hand-rolling a rate:

  `python3 scripts/verify-ledger.py sccache-report --ledger-dir data/verify-logs --min-samples 10`

  It also counts the samples it discards (`sccache-unavailable`,
  `no-concurrency`, `unparseable-stats`, `counter-reset`).
- A per-verify `SCCACHE_DIR` or a second daemon stays **rejected**: it would
  fragment the shared cache.

### Seed → first build

- **Take the FIRST record per `seed_id`.** Every later verify in that lane
  repeats the same `seed` object, because the block is non-destructive.
- **The agent's first cargo is not observable, by ruling.** Leo's 2026-10-13
  ruling on esc-7424-14: reify's agent CPU governance stays **OFF**, so
  `cpu_governance.exec_path` and `shim_dir` will not be set, and
  `scripts/agent-bin/cargo` is not on agents' PATH. Its start log stays inert,
  though exec-transparent, until activation. Activation is owned by #5908,
  which is blocked on dark_factory:3394.
- **So an agent's first build appears ONLY as a bounded-but-unattributed
  window**: `unattributed_writes`, spanning `[seed.exited_at, first known
  cargo start)`. `first_build` is verify-observed (`basis:
  "verify-step-end"`), and `unattributed_writes.last_write_at −
  seed.exited_at` is the closest proxy for the agent's tax.
- **An unattributed window is NEVER zero agent cost.** A `basis:
  "verify-step-end"` `first_build` in a lane whose
  `unattributed_writes.units > 0` is not the lane's first build. Report such
  seeds separately, bounded by the unattributed span, instead of folding them
  into the verify-first figure.
- **`units_written: 0`** means the first observed cargo wrote no fingerprint.
  Read `fingerprint.units_written_since_seed` and `last_write_at` for the
  lane-wide picture.
- **For D6:** the ledger records wall seconds, not CPU. The study's
  core-equivalent multiplies by the build's job count (`-j 8`), which is an
  upper bound.
- **D6's "≥ 1 time-averaged core" figure carries the unattributed-window
  limitation above.** Every E2/D6 write-up must say so.

## Known blind spots

- **Node lane:** it runs in the background between `gui_node_launch` and
  `gui_node_join`, so `join.ended_at − launch.began_at` is an upper bound on
  the vitest block, not its wall.
- **Non-pool `run_all.sh` paths** (`host-infra`, `member-subset`,
  `legacy-serial`) write a summary but no member timing.
- **A run killed with SIGKILL** (or any kill that skips the traps) leaves no
  record.
- **Fingerprint scan scope:** only `target/debug` and `target/release`;
  cross-target triples are not scanned.
- **Concurrency counts are point samples** at the two edges of a step, not
  integrals over it.

## Growth

These sizes are estimates.

- **Merge-gate record: about 50–60 KB.** The merge plan has 22 entries, 7 of
  them cargo-bearing, giving 14 sccache objects of about 1.2 KB each. About
  200 `run_all` member rows add about 150 B each.
- **Task-scope record: about 5–20 KB.**
- **At the study's volumes** (24–51 acquires/day, a few verifies each, plus
  merge gates), that is a few MB/day, or roughly 100–150 MB per 30 days.
- **Rotation:** nothing rotates automatically. Rotate by renaming the file:
  each append opens it by path under the lock.

## Query recipes

Run these from the main checkout. `L` is the ledger file.

```bash
L=data/verify-logs/verify-runs.jsonl
```

**E1: median wall per phase, passing merge gates:**

```bash
python3 -c 'import json,sys,statistics as st,collections as c; W=c.defaultdict(list); [W[s["phase"]].append(s["wall_secs"]) for r in map(json.loads, open(sys.argv[1])) if r["role"]=="merge" and r["verdict"]=="pass" for s in r["steps"] if s["wall_secs"] is not None]; [print(p, len(v), round(st.median(v), 1)) for p, v in sorted(W.items())]' "$L"
```

**E1: debug nextest build+listing vs execution, passing merge gates:**

```bash
python3 -c 'import json,sys,statistics as st; N=[s["nextest"] for r in map(json.loads, open(sys.argv[1])) if r["role"]=="merge" and r["verdict"]=="pass" for s in r["steps"] if s["phase"]=="debug_nextest" and "exec_secs" in s.get("nextest", {})]; print(len(N), N and st.median(n["pre_exec_secs"] for n in N), N and st.median(n["exec_secs"] for n in N))' "$L"
```

**E1: infra pool serial-phase wall and the three slowest members per gate:**

```bash
python3 -c 'import json,sys; [print(r["run_id"], round(p["serial"]["ended_at"]-p["serial"]["began_at"], 1), sorted((round(m["ended_at"]-m["started_at"], 1), m["name"]) for m in a["members"] if m["started_at"] and m["ended_at"])[-3:]) for r in map(json.loads, open(sys.argv[1])) if r["role"]=="merge" for s in r["steps"] for a in [s.get("run_all") or {}] for p in [(a.get("summary") or {}).get("phases")] if p and p["serial"]["began_at"] and p["serial"]["ended_at"]]' "$L"
```

**E1 / E12: sccache hit rate by concurrency bucket (ruling D10):**

```bash
python3 scripts/verify-ledger.py sccache-report --ledger-dir "$(dirname "$L")" --min-samples 10
```

**E2 / D6: seed → first build, first record per seed:**

```bash
python3 -c 'import json,sys; F={}; [F.setdefault(r["lane_seed"]["seed"]["seed_id"], r["lane_seed"]) for r in map(json.loads, open(sys.argv[1])) if r.get("lane_seed")]; [print(k, b.get("source"), b.get("basis"), b.get("seed_to_first_build_secs"), v["unattributed_writes"]["last_write_at"] and round(v["unattributed_writes"]["last_write_at"]-v["seed"]["exited_at"], 1), v["fingerprint"]["units_written_since_seed"]) for k, v in F.items() for b in [v["first_build"] or {}]]' "$L"
```

**E11 / E12: debug nextest `pre_exec_secs` before vs after a cutover epoch**
(E12: group by `steps[].sccache.before.reason == "rustc-wrapper-not-sccache"`
instead):

```bash
python3 -c 'import json,sys,statistics as st; T=float(sys.argv[2]); R=[(r["started_at"], s["nextest"]["pre_exec_secs"]) for r in map(json.loads, open(sys.argv[1])) if r["role"]=="merge" and r["verdict"]=="pass" for s in r["steps"] if s["phase"]=="debug_nextest" and "pre_exec_secs" in s.get("nextest", {})]; [print(k, len(v), v and st.median(v)) for k, v in (("before", [x for t, x in R if t < T]), ("after", [x for t, x in R if t >= T]))]' "$L" <cutover-epoch>
```

**E13: release-skip firing rate, merge gates:**

```bash
python3 -c 'import json,sys,collections as c; print(c.Counter(r["release_pass"]["decision"] for r in map(json.loads, open(sys.argv[1])) if r["role"]=="merge"))' "$L"
```
