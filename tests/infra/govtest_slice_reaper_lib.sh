#!/usr/bin/env bash
# tests/infra/govtest_slice_reaper_lib.sh — lifecycle for the private
# per-run systemd slices created by tests/infra/test_cpu_load_governance.sh
# (task 5930). Designed to be sourced by that test and by
# tests/infra/test_govtest_slice_reaper.sh.
#
# Functions:
#   govtest_slice_pid <unit>       echo the embedded pid, or nothing if <unit>
#                                  is outside the govtest name grammar
#   govtest_slice_units <pid>      echo this run's three unit names, one per
#                                  line, in TEARDOWN order (children, parent)
#   govtest_stale_units <self_pid> <listing>
#                                  filter a `systemctl --user list-units`
#                                  listing down to one PARENT unit name per
#                                  dead predecessor run
#   govtest_reap_stale [<self_pid>] enumerate + stop every dead predecessor's
#                                  slice; no-op when systemctl is absent
#   govtest_slice_teardown <pid>   unconditionally stop this run's own three
#                                  slices, children before parent
#
# Knobs:
#   REIFY_GOVTEST_TEST_MODE             set to 1 to ARM the test seams below.
#                                       Nothing else in the repo sets it, so
#                                       the seams are inert on every real run.
#   REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS  space-separated pid list that replaces
#                                       the `kill -0` liveness oracle (test
#                                       seam; mirrors the REIFY_CPU_GOV_TEST_*
#                                       idiom in test_cpu_load_governance.sh).
#                                       Honoured ONLY when
#                                       REIFY_GOVTEST_TEST_MODE=1 — see
#                                       _govtest_pid_alive for why a liveness
#                                       seam needs a second key.
#
# WHY THIS LIVES IN tests/infra/ AND NOT scripts/lib_cgroup.sh
# The `reify-govtest` prefix is test-private BY CONSTRUCTION:
# test_cpu_load_governance.sh requires its slice names to differ from the
# production `reify-governed-{agents,merge}.slice` so that its usage_usec
# deltas stay isolated from concurrent real agent placement (ζ). Meanwhile
# scripts/lib_cgroup.sh is sourced by scripts/cpu-governed-exec.sh on EVERY
# governed exec, so teaching that production hot-path library about a
# test-only slice prefix would be a layering inversion — production code
# would carry knowledge of, and a stop path for, units only tests create.
# tests/infra/*_lib.sh is the established house pattern for logic shared
# between infra tests (cpu_load_fixture.sh, load_tolerance_lib.sh,
# nextest_absent_lib.sh, run-all-classification-lib.sh). It is also not a
# `test_*.sh`, so run_all.sh's auto-discovery skips it and it needs no
# run-all-classification.manifest row.

# Source guard — prevent double-sourcing (mirrors lib_portable.sh /
# lib_cgroup.sh / lib_test_semaphore.sh).
if [ "${_REIFY_GOVTEST_SLICE_REAPER_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_GOVTEST_SLICE_REAPER_LIB_SOURCED=1

# ---------------------------------------------------------------------------
# govtest_slice_pid <unit>
#   Echo the pid embedded in a govtest slice unit name, or NOTHING when the
#   name is outside the grammar
#
#       ^reify-govtest([0-9]+)(-agents|-merge)?\.slice$
#
#   EMPTINESS IS THE ONLY SIGNAL — this function always returns 0. Callers
#   run under `set -euo pipefail`, and a non-zero return from inside a
#   `pid="$(govtest_slice_pid "$u")"` capture is an abort hazard at every
#   call site for no benefit; testing the captured string is equivalent and
#   cannot misfire.
#
#   The grammar is deliberately EXACT rather than a prefix test. It is the
#   single chokepoint deciding whether a unit is eligible to be stopped, and
#   the per-user systemd session it operates in is shared host-wide with the
#   production `reify-governed-{agents,merge}.slice` units that carry real
#   agent placement. An exact anchored match is what makes those units
#   unreachable from here by construction. This is the same defensive
#   re-filter discipline dark-factory's leftover-scope reaper applies at
#   verify.py:3503 — it re-checks the full name with an anchored regex even
#   though it already enumerated by prefix glob, so a surprise in glob
#   semantics can never widen the blast radius.
# ---------------------------------------------------------------------------
govtest_slice_pid() {
    local unit="${1:-}"
    if [[ "$unit" =~ ^reify-govtest([0-9]+)(-agents|-merge)?\.slice$ ]]; then
        printf '%s\n' "${BASH_REMATCH[1]}"
    fi
    return 0
}

# ---------------------------------------------------------------------------
# govtest_slice_units <pid>
#   Echo the three unit names a single test_cpu_load_governance.sh run owns,
#   one per line: the -agents child, the -merge child, then the bare parent.
#
#   THE ORDER IS TEARDOWN ORDER — children first, parent last. It preserves
#   the ordering rationale already carried by that script's _cleanup_all,
#   which stops the confined-quota parent LAST, after its children, to avoid
#   leaving a quota'd empty parent unit behind.
#
#   The names are fully determined by the pid, which is why teardown needs no
#   record of what was actually created (see govtest_slice_teardown).
# ---------------------------------------------------------------------------
govtest_slice_units() {
    local pid="${1:-}"
    printf 'reify-govtest%s-agents.slice\n' "$pid"
    printf 'reify-govtest%s-merge.slice\n' "$pid"
    printf 'reify-govtest%s.slice\n' "$pid"
    return 0
}

# ---------------------------------------------------------------------------
# _govtest_pid_alive <pid>
#   Internal. Return 0 if <pid> is a live process, non-zero otherwise.
#
#   When REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS is set and non-empty AND
#   REIFY_GOVTEST_TEST_MODE=1, it REPLACES the real oracle with a
#   word-membership test against that list. The seam exists solely so the
#   reaper's liveness logic is testable without real host pids: a hermetic
#   test cannot make a chosen pid dead on demand, and picking a "surely dead"
#   fixture pid is exactly the kind of host-dependence that turns a pool
#   member into a flake. This is the same environment-fixture idiom
#   test_cpu_load_governance.sh already uses about ten times over
#   (REIFY_CPU_GOV_TEST_PROC_PATH, REIFY_CPU_GOV_TEST_CONFINE_CPUS,
#   REIFY_CPU_ADMIT_MEM_PROC_PATH, ...) and lib_cgroup.sh uses for
#   REIFY_CPU_GOVERN_CONTROLLERS_PATH.
#
#   WHY THIS SEAM NEEDS A SECOND KEY, UNLIKE THOSE. Every seam listed above
#   redirects a READ to a fixture; the worst a stray one can do is make a
#   test measure the wrong file. This one replaces the oracle that decides
#   what gets STOPPED, and it does so on the PRODUCTION path —
#   govtest_reap_stale runs at the top of every real test_cpu_load_governance
#   .sh run. A stray non-empty pid list would make every govtest pid outside
#   it read DEAD, which does not merely weaken the fail-safe direction
#   documented on govtest_stale_units, it INVERTS it: the sweep would stop a
#   live concurrent lane's parent slice mid-measurement. Requiring
#   REIFY_GOVTEST_TEST_MODE=1 as well means the fake oracle can only engage
#   under a marker no injection source sets and no production path passes,
#   so on a real run `kill -0` is the only oracle reachable — whatever else
#   happens to be exported.
# ---------------------------------------------------------------------------
_govtest_pid_alive() {
    local pid="${1:-}"
    [ -n "$pid" ] || return 1
    if [ "${REIFY_GOVTEST_TEST_MODE:-0}" = "1" ] \
        && [ -n "${REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS:-}" ]; then
        local fake
        for fake in ${REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS}; do
            [ "$fake" = "$pid" ] && return 0
        done
        return 1
    fi
    kill -0 "$pid" 2>/dev/null
}

# ---------------------------------------------------------------------------
# govtest_stale_units <self_pid> <listing>
#   Echo one PARENT unit name — `reify-govtest<pid>.slice` — per dead
#   predecessor run found in <listing>, which is raw output of
#   `systemctl --user list-units --all --plain --no-legend` (unit name in
#   field 1). Emission is in first-seen order; always returns 0.
#
#   ONE LINE PER RUN, NOT PER UNIT. Measured directly rather than inferred
#   from systemd docs: a throwaway child slice was created under a parent,
#   the PARENT ALONE was stopped, and BOTH units then vanished from
#   `systemctl --user list-units --all`. Stopping the parent cascades, so a
#   leaked run's three unit names must collapse to one action — which is why
#   deduplication by pid is part of this function's contract rather than an
#   optimisation, and why there is no ordering hazard from stopping a parent
#   whose children are still listed.
#
#   FAIL-SAFE IN EXACTLY ONE DIRECTION. A pid is skipped when it is alive,
#   when it is the caller's own, or when the unit name does not parse — and
#   the name check runs through govtest_slice_pid, so the production
#   `reify-governed-*` slices and any foreign unit the enumeration glob might
#   surprise us with are dropped here too. The only error this design can
#   make is a false NEGATIVE (pid reuse: an unrelated process now holds a
#   dead run's pid), which merely leaves one empty slice behind for the next
#   sweep to retry. It can never reap a live concurrent run — which matters
#   because run_all.sh schedules many lanes at once against ONE shared
#   per-user systemd session.
#
#   Dedup uses a plain space-delimited seen-list rather than an associative
#   array: the candidate count is bounded by the number of leaked runs (a
#   handful), and this keeps the library free of a bash-4 dependency.
# ---------------------------------------------------------------------------
govtest_stale_units() {
    local self_pid="${1:-}" listing="${2:-}"
    local line unit pid seen=" " emitted

    while IFS= read -r line; do
        # Field 1 is the unit name; `read` also drops blank/whitespace-only
        # rows here, since unit ends up empty for them.
        read -r unit _ <<EOF2
$line
EOF2
        [ -n "$unit" ] || continue

        pid="$(govtest_slice_pid "$unit")"
        [ -n "$pid" ] || continue                 # not a govtest unit
        [ "$pid" = "$self_pid" ] && continue      # never our own run
        _govtest_pid_alive "$pid" && continue     # never a live run

        case "$seen" in
            *" $pid "*) continue ;;               # already emitted this run
        esac
        seen="$seen$pid "

        emitted="reify-govtest${pid}.slice"
        printf '%s\n' "$emitted"
    done <<EOF
$listing
EOF

    return 0
}

# ---------------------------------------------------------------------------
# govtest_reap_stale [<self_pid>]
#   CRASH RECOVERY. Enumerate this project's govtest slices, and stop the
#   parent of every run whose pid is dead. Defaults <self_pid> to `$$`.
#   Always returns 0.
#
#   WHY A STARTUP SWEEP IS NEEDED AT ALL. test_cpu_load_governance.sh tears
#   its own slices down from an EXIT trap, and that trap is more robust than
#   it looks — measured on this host, a bash script blocked in a foreground
#   command runs its EXIT trap on SIGTERM, SIGINT and SIGHUP alike. Only
#   SIGKILL skips it. So widening the trap to `EXIT INT TERM HUP` would
#   change nothing; the residue this sweep exists to clean is precisely the
#   uncatchable one — a verify timeout, a harness reap, or an OOM kill.
#   Recovering at the next start is the same shape dark-factory's
#   leftover-verify-scope reaper uses (harness.py `_run_leftover_verify_scope
#   _reaper_pass` -> verify.py `reap_leftover_verify_scopes`).
#
#   TWO BOUNDS ON THE BLAST RADIUS, BOTH DELIBERATE. The enumeration glob
#   `reify-govtest*.slice` is the OUTER bound: the systemd user session is
#   shared host-wide, so listing every unit and filtering afterwards would
#   put the production `reify-governed-*` slices — and every other project's
#   units — inside the candidate set in the first place. The anchored
#   grammar re-filter inside govtest_slice_pid is the INNER bound, applied
#   even though the glob already ran. Keeping both is exactly the
#   belt-and-braces of dark-factory verify.py:3492 (glob) + :3503 (anchored
#   regex re-check), and it means a surprise in systemctl's glob semantics
#   cannot widen what gets stopped.
#
#   FAIL-SOFT THROUGHOUT. A missing systemctl returns immediately; a failing
#   one is swallowed. This function is called at the top of a suite running
#   under `set -euo pipefail`, so any escaping non-zero status would abort
#   the whole governance run before a single row executed and surface as a
#   governance regression that isn't one.
#
#   Each reap is announced on stdout rather than done silently, so a sweep is
#   visible in the governance suite's transcript — the same log-what-you-
#   reaped discipline the dark-factory sweep follows.
# ---------------------------------------------------------------------------
govtest_reap_stale() {
    local self_pid="${1:-$$}"
    local listing unit

    command -v systemctl >/dev/null 2>&1 || return 0

    listing=""
    # `|| true` inside the capture AND a `|| listing=""` outside: the former
    # covers a non-zero systemctl exit, the latter covers the assignment
    # itself failing under `set -e`.
    listing="$(systemctl --user list-units --all --plain --no-legend \
        'reify-govtest*.slice' 2>/dev/null || true)" || listing=""

    [ -n "$listing" ] || return 0

    # `</dev/null` on the stop is STRUCTURAL, not cosmetic: this loop's stdin
    # IS the heredoc carrying the remaining units, so a callee that read from
    # stdin would swallow them and silently reap only the first of several
    # leaked runs. systemctl does not read stdin today; detaching it means a
    # future one — or a shim/wrapper placed on PATH — cannot turn this into a
    # truncated sweep. Same reason on govtest_slice_teardown's loop.
    while IFS= read -r unit; do
        [ -n "$unit" ] || continue
        systemctl --user stop "$unit" </dev/null 2>/dev/null || true
        echo "  reaped stale govtest slice: $unit"
    done <<EOF
$(govtest_stale_units "$self_pid" "$listing")
EOF

    return 0
}

# ---------------------------------------------------------------------------
# govtest_slice_teardown <pid>
#   Stop this run's own three slices — children first, parent last —
#   UNCONDITIONALLY. Always returns 0. Safe to call from an EXIT trap.
#
#   WHY UNCONDITIONAL RATHER THAN FLAG-GUARDED. The mechanism this replaces
#   recorded what it had created in `_ROW4_SLICE_TASK_CREATED` /
#   `_ROW4_SLICE_MERGE_CREATED` / `_ROW4_CONFINE_PARENT_CREATED` and stopped
#   only what those named. That is a drift-prone design: it re-derives at 8
#   scattered assignment sites something `$$` already determines before any
#   test cycle runs. And it had drifted — `_row4_confine_apply_quota` vivifies
#   the parent slice from FOUR call sites but only TWO set the parent flag,
#   and those two sit behind branches that additionally require `taskset` and
#   a readable Cpus_allowed_list. On a host with cgroup governance but no
#   `taskset`, the two unguarded sites still created the parent and nothing
#   ever stopped it — a leak on a fully GREEN exit, distinct from the SIGKILL
#   leak govtest_reap_stale handles.
#
#   Unconditional teardown closes that whole class rather than the one
#   instance: the three names are fully determined by the pid, and
#   `systemctl --user stop` on a never-started slice is a harmless no-op
#   already swallowed by the `|| true` this function keeps. So there is
#   nothing left to remember, and no future call site can forget to.
#
#   Stopping the PARENT alone would in fact suffice — cascade was measured
#   (see govtest_stale_units). The children are stopped first purely as
#   belt-and-braces for a host where cascade semantics differ, and the
#   children-then-parent order preserves the rationale the previous
#   _cleanup_all already carried: never leave a quota'd empty parent behind.
#
#   Returns 0 even when systemctl is missing or failing. This runs inside an
#   EXIT trap, where a non-zero return would overwrite the governance suite's
#   real exit status and report a passing run as failed.
# ---------------------------------------------------------------------------
govtest_slice_teardown() {
    local pid="${1:-$$}"
    local unit

    command -v systemctl >/dev/null 2>&1 || return 0

    # `</dev/null` — see govtest_reap_stale: the loop's stdin is the heredoc
    # listing the units still to stop, so a stdin-consuming callee would strand
    # the parent slice, which is the one that actually matters.
    while IFS= read -r unit; do
        [ -n "$unit" ] || continue
        systemctl --user stop "$unit" </dev/null 2>/dev/null || true
    done <<EOF
$(govtest_slice_units "$pid")
EOF

    return 0
}
