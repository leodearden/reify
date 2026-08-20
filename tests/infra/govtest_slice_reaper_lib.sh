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
#
# Knobs:
#   (none yet)
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
