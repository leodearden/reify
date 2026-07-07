# Orphaned Test-Binary Reaper — Design & Cross-Repo Seam Contract

**Task #4872 | 2026-06-27**

---

## Problem

When a `verify.sh` run's cargo/nextest parent is killed abnormally — orchestrator
cancel, command-timeout SIGKILL, OOM-killer, manual kill — in-flight nextest test
binaries are **not reaped**.  nextest's own slow-timeout SIGKILL only fires from a
live parent process; once the parent is gone the test processes **reparent to PID 1
/ systemd --user** and survive indefinitely, holding RAM and swap.

**Incident (2026-06-26):** two `reify_fdm` test processes survived for 16.5h after
their parent was SIGKILLed, holding ~143 GiB of swap on the shared 125 GiB merge
box.  The `reify_fdm` FDM-allocation root cause was fixed on main (82aca0ef2c); this
task fixes the generic leaked-orphan **survival** mechanism, which recurs with any
wedged or bloated test whenever the verify parent dies.

---

## Root Cause

1. SIGKILL to verify.sh's PID does not propagate to children.
2. verify.sh cannot run any trap handler on SIGKILL.
3. Nothing placed the cargo subtree in a dedicated, teardown-able process group.

For graceful kills (SIGTERM/SIGINT) a trap *can* run — but prior to this task no
such trap killed the cargo subtree.

---

## Two-Layer Fix

### Layer A — In-process process-group teardown (verify.sh)

`scripts/verify.sh` now routes all `cargo nextest run` and `cargo test` plan steps
through `reaper_run_in_pgroup` from `scripts/lib_proc_reaper.sh`.

`reaper_run_in_pgroup` uses the `set -m; eval cmd &; PGID=$!; set +m` idiom (the
same proven idiom as `portable_timeout()` in `scripts/lib_portable.sh`) to run each
cargo pass as its own **process-group leader**.  The PGID is tracked in a module
array so `reaper_teardown` can SIGTERM → grace → SIGKILL the entire group atomically
via `kill -- -<pgid>`.

`reaper_teardown` is called:
- From `_verify_cleanup` (which runs on EXIT).
- From explicit `trap '_verify_cleanup; exit N' INT TERM HUP`.

This closes the **graceful-cancel window** — the orchestrator typically SIGTERMs
before escalating, so Layer A catches the common case.

### Layer B — Host-wide orphan reaper (SIGKILL case)

`scripts/lib_proc_reaper.sh reap-orphans` (exposed via the thin wrapper
`scripts/reap-orphaned-test-binaries.sh`) scans running processes and SIGKILLs those
that match **all four conditions**:

| Condition | Details |
|---|---|
| **Exe path** | Resolved `/proc/<pid>/exe` matches `REIFY_REAPER_DEPS_GLOB` |
| **Orphan PPID** | PPID == 1, OR parent comm in `REIFY_REAPER_COMMS` (`systemd init`) |
| **Age** | `etimes` ≥ `REIFY_REAPER_MIN_AGE_SECS` (default 7200 s) |
| **UID** | Owned by `REIFY_REAPER_UID` (default current user) |

The **PPID/init-comm condition is the primary safety gate**: a LIVE nextest test
binary has PPID=cargo/nextest (never PID 1/systemd) and runs < 2h, so it can never
satisfy all four conditions simultaneously.  The reaper **cannot kill an in-flight
verify run**.

`--dry-run` mode reports candidates without killing them.

---

## Implementation

| File | Role |
|---|---|
| `scripts/lib_proc_reaper.sh` | Sourceable lib + direct-exec main-guard (`reap-orphans` subcommand) |
| `scripts/reap-orphaned-test-binaries.sh` | Thin cron/seam entry-point wrapper |
| `scripts/verify.sh` | Source + executor routing + _verify_cleanup + traps |
| `tests/infra/test_proc_reaper.sh` | Unit + integration tests |

---

## Knobs

| Variable | Default | Purpose |
|---|---|---|
| `REIFY_REAPER_GRACE_SECS` | `10` | SIGTERM→SIGKILL grace in in-process teardown |
| `REIFY_PROC_REAPER_DISABLE` | — | Set to `1` to bypass in-process teardown (break-glass) |
| `REIFY_REAPER_DEPS_GLOB` | `*/target/debug/deps/* */target/release/deps/*` | Glob for candidate exe paths |
| `REIFY_REAPER_MIN_AGE_SECS` | `7200` | Minimum age in seconds for host-wide sweep |
| `REIFY_REAPER_ORPHAN_PPIDS` | `1` | Space-separated PPIDs considered orphan parents |
| `REIFY_REAPER_COMMS` | `systemd init` | Space-separated comm names of orphan-parent procs |
| `REIFY_REAPER_UID` | `$(id -u)` | UID to scope the sweep to |

---

## Cross-Repo Seam Contract

Reify ships Layer A (verify.sh in-process teardown) and Layer B's primitive
(`scripts/reap-orphaned-test-binaries.sh`).  The durable SIGKILL fix requires the
**killer** — the orchestrator's cancel/timeout handler — to target the verify process
group and/or run the reaper sweep after a cancel.  This is a dark-factory change.

**Dark-factory is expected to wire:**

1. **On cancel/timeout:** `kill -- -<verify_pgid>` (targeting the verify.sh process
   group, not just verify.sh's own PID) — makes Layer A's teardown trap fire even
   for orchestrator-issued kills, without needing an external reaper.

2. **Periodic sweep (cron or post-cancel hook):**
   ```bash
   bash /path/to/scripts/reap-orphaned-test-binaries.sh
   ```
   With appropriate `REIFY_REAPER_*` knobs (e.g. `REIFY_REAPER_UID=$(id -u)`).
   This is the **only mechanism** that catches orphans from true SIGKILL to the
   verify parent — Layer A's traps cannot run after SIGKILL.

This seam is the same class as the cpu-governance α/β/γ↔ζ seam and the warm-lane
D8/D10 seam — reify ships the primitive and the contract; dark-factory wires the
invocation.

---

## Testing

`tests/infra/test_proc_reaper.sh` (auto-discovered by `tests/infra/run_all.sh`)
covers:

- **Part 1:** `reaper_kill_pgroup` — structural (uses `kill -- -<pgid>` form), kills
  leader AND child in one group, ESRCH-safe on stale PGID.
- **Part 2:** `reap-orphans` — positive (kills matching orphan), negative (PPID not
  in set / age below threshold / not under deps glob / dry-run).
- **Part 3:** `reaper_run_in_pgroup` / `reaper_teardown` — exit-code propagation,
  SIGTERM teardown kills entire group, idempotent double-teardown.
- **Part 4:** Structural verify.sh wiring — sources lib, routes cargo passes through
  pgroup runner, TERM trap, `_verify_cleanup` calls teardown, no plan-churn.
- **Part 5:** End-to-end SIGKILL proof — parent SIGKILLed; orphan survives; wrapper
  script reaps it.
- **Part 9:** esc-5020 run_all false-kill rule-out — reconstructs the live
  `run_all.sh` pool topology and asserts the reaper spares an ancestry-intact
  deps-glob child, with a non-vacuity companion proving the same fixture IS
  reaped once its PPID is in the orphan set. See below.

---

## esc-5020 — run_all false-kill: RULED OUT

**Task #5127 | 2026-07-07**

**Question:** can the host-wide reaper false-kill a live `tests/infra/run_all.sh`
job under host overload? **Ruled out.** Audited against the live run_all Phase-1
pool topology (`( slot_acquire; bash INFRA_DIR/test_*.sh 9<&- ) &` subshells
joined by `wait`; no `setsid`/`systemd-run`/`nohup`/`disown` anywhere in
`run_all.sh`, so ancestry stays intact for as long as a job is live).

A kill requires all **four** conditions in the table above at once, and the
live run_all topology fails at least one of them for as long as it is
genuinely live:

- **Exe path:** a live pool subshell and its `bash test_*.sh` child resolve
  `/proc/<pid>/exe` to the bash interpreter, which never matches
  `REIFY_REAPER_DEPS_GLOB` — fails the exe-path gate outright.
- **Orphan PPID:** the only run_all-subtree processes whose exe *does* match
  the deps glob are compiled binaries a test launches under a
  `target/*/deps/*` path (e.g. `test_proc_reaper.sh`'s own hermetic
  `cp $(command -v sleep)` fixtures). While live, these have PPID = the
  launching test's bash — never `1`, comm=`bash`, never `systemd`/`init` —
  which fails the orphan-PPID/ancestry gate.
- **Age:** a whole run_all invocation completes in minutes (pool `wait`
  deadline 1800s soft; test sentinels self-expire ≤600s), far under the
  default 7200s `REIFY_REAPER_MIN_AGE_SECS`.

A process satisfies all four only **after its ancestry breaks** (PPID → 1) —
i.e. it is a genuine leaked orphan of a **completed** test, and reaping it at
that point is the reaper's intended purpose, not a false-kill.

Host overload does not weaken this: the host-wide `ps` scan is bounded by
`REIFY_REAPER_PS_TIMEOUT` (default 15s) and **skips the sweep cycle on
timeout** (esc-4889-94) instead of proceeding on partial data, so under load
the reaper becomes more conservative, not less. `run_all.sh` never sources
`lib_proc_reaper.sh`, so Layer A teardown is never in play for it — only the
Layer-B host-wide sweep can observe run_all processes at all, and it is
gated as above.

**Rejected fix — session/pgid-trace exclusion.** The escalation proposed
sparing any process whose session/pgid traces back to a live run_all
invocation. Rejected: sessions do not change on reparenting, so a *genuine*
leaked orphan (PPID → 1, age ≥ 7200s) retains the SID of the run_all
invocation that originally spawned it. Keying sparing on session-liveness
would therefore SPARE genuine leaked orphans whenever *any* run_all happens
to be concurrently alive on the shared merge box — regressing the
leaked-orphan-survival fix this reaper exists to provide. Correct sparing
must key on ancestry-**intactness** (a live, non-init parent), which the
existing orphan-PPID gate already enforces precisely; no session/pgid
predicate was added.

**Locked by test, made auditable.** The rule-out is locked by
`test_proc_reaper.sh` Part 9 (a reconstructed run_all-topology fixture that
must always be spared) and made observable during a live run_all via a
`--dry-run`-only diagnostic: `reap-orphans --dry-run` now reports
`spared (non-orphan/live ancestry): pid=... exe=... ppid=... ppid_comm=...`
for any deps-glob candidate spared by the orphan-PPID gate. The diagnostic is
dry-run-only and reaper-agnostic (no run_all coupling in `lib_proc_reaper.sh`
itself) — the real sweep stays silent so it doesn't spam one line per live
deps-glob binary on a busy host.
