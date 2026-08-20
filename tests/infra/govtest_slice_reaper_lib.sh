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
#
# Knobs:
#   REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS  space-separated pid list that replaces
#                                       the `kill -0` liveness oracle (test
#                                       seam; mirrors the REIFY_CPU_GOV_TEST_*
#                                       idiom in test_cpu_load_governance.sh)
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
#   When REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS is set and non-empty it REPLACES
#   the real oracle with a word-membership test against that list. The seam
#   exists solely so the reaper's liveness logic is testable without real host
#   pids: a hermetic test cannot make a chosen pid dead on demand, and picking
#   a "surely dead" fixture pid is exactly the kind of host-dependence that
#   turns a pool member into a flake. This is the same environment-fixture
#   idiom test_cpu_load_governance.sh already uses about ten times over
#   (REIFY_CPU_GOV_TEST_PROC_PATH, REIFY_CPU_GOV_TEST_CONFINE_CPUS,
#   REIFY_CPU_ADMIT_MEM_PROC_PATH, ...) and lib_cgroup.sh uses for
#   REIFY_CPU_GOVERN_CONTROLLERS_PATH.
# ---------------------------------------------------------------------------
_govtest_pid_alive() {
    local pid="${1:-}"
    [ -n "$pid" ] || return 1
    if [ -n "${REIFY_GOVTEST_REAP_FAKE_ALIVE_PIDS:-}" ]; then
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
