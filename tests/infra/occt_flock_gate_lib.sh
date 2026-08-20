#!/usr/bin/env bash
# Shared serialization-timing bounds for OCCT flock-gate Tests 20 and 21B
# (tests/infra/test_occt_flock_gate.sh).
#
# WHY A SHARED LIB:
# Tests 20 and 21B both spawn 3 concurrent wrapper invocations with N=2 slots
# and assert wall-clock is within [LOW,HIGH]ms to prove the 3rd was serialized.
# Extracting the constants and predicate here ensures the two tests cannot drift
# out of sync (one source of truth) and makes the bounds unit-testable with
# synthetic inputs (see test_occt_flock_gate_bounds.sh), avoiding another
# sleep-based timing test that could itself flake under load.
#
# UPPER BOUND RATIONALE (esc-3939-94):
# The original upper bound of 1200ms was raised to 2000ms because the merge-queue
# verify pipeline runs concurrently with cargo clippy + OCCT/GUI builds, inflating
# process-spawn and flock-acquire latency of the serialized 3rd invocation.
# An observed run measured 1473ms (FAIL) while its semantically identical twin
# Test 21B measured 948ms (PASS) in the SAME run — non-determinism, not a logic
# defect. On an idle host both pass deterministically (2026-05-30: Test 20=984ms,
# Test 21B=939ms, 41 passed/0 failed).
#
# At 2000ms the upper bound no longer discriminates N=2 (~800ms) from fully-serial
# N=1 (~1200ms); it becomes a load-tolerant sanity ceiling that still flags gross
# wedges (a true hang lands in LOCK_WAIT/timeout territory, orders of magnitude
# larger). The >=700ms LOWER bound guards against under-serialization only
# (all-parallel N>=3 finishes ~400ms).
#
# COVERAGE GAP (accepted tradeoff per esc-3939-94): no test in this suite currently
# detects an over-serialization regression (N collapsing to 1, producing ~1200ms for
# three invocations — inside [700,2000], undetected). Test 19 does NOT cover this
# case: two fully-serial invocations complete in ~800ms, below Test 19's own <2000ms
# threshold, so Test 19 also passes under a fully-serial regression.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_OCCT_FLOCK_GATE_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_OCCT_FLOCK_GATE_LIB_SH_SOURCED=1

# Lower bound (ms): proves the 3rd invocation was serialized.
# All-parallel N>=3 finishes ~400ms; >=700ms means at least one invocation waited.
OCCT_SERIAL3_N2_LOW_MS=700

# Upper bound (ms): load-tolerant sanity ceiling, raised 1200->2000->5000 per esc-3939-94.
# Observed 3317ms (Test 21B) under task/3443 verify load: process-spawn latency for
# `timeout … bash -c 'sleep 0.4'` inflated slot hold-time beyond 2000ms with no
# logic defect.  5000ms still flags gross wedges (a true hang is LOCK_WAIT territory:
# minutes, not seconds) while avoiding spurious failures under heavy verify-pipeline
# concurrency.
OCCT_SERIAL3_N2_HIGH_MS=5000

# occt_serial3_n2_within_bounds MS
# Returns 0 (success) if MS is in [OCCT_SERIAL3_N2_LOW_MS, OCCT_SERIAL3_N2_HIGH_MS].
# Returns 1 (failure) otherwise.
occt_serial3_n2_within_bounds() {
    local ms="$1"
    [ "$ms" -ge "$OCCT_SERIAL3_N2_LOW_MS" ] && [ "$ms" -le "$OCCT_SERIAL3_N2_HIGH_MS" ]
}

# occt_max_concurrent_holders EVENT_LOG
# R-technique predicate (PRD docs/prds/infra-test-wallclock-deflake.md §2/T3).
# Reads a slot event log (REIFY_SLOT_EVENT_LOG format from
# scripts/lib_slot_acquire.sh) and echoes to stdout the maximum number of
# slots held simultaneously across all events recorded in the log.
#
# Log line format:
#   <epoch_ns> <pid> ACQUIRE slot-N   (emitted by slot_acquire on success)
#   <epoch_ns> <pid> RELEASE          (emitted by caller before closing FD 9)
#
# Why sort -n by the leading epoch-ns field (NOT physical line order):
#   Concurrent wrapper PIDs write via O_APPEND (atomic EoF appends), but the
#   OS may schedule competing appends in any order, so physical line order may
#   differ from nanosecond-timestamp order.  The CAUSAL ORDERING INVARIANT in
#   scripts/lib_slot_acquire.sh guarantees ts(prev RELEASE) < ts(next ACQUIRE),
#   so ns-sorted order is the canonical causal sequence.
#
# Echoes an integer >= 0.  Empty log or RELEASE-only log → 0.
occt_max_concurrent_holders() {
    local _log="$1"
    sort -n "$_log" | awk '
        $3 == "ACQUIRE" { c++; if (c > m) m = c }
        $3 == "RELEASE" { c-- }
        END { print m+0 }
    '
}

# occt_wait_until_slot_held SLOT_FILE [MAX_ITERS=100]
# Causal flock-probe barrier (PRD docs/prds/merge-gate-health.md W4b, task 5258).
#
# Polls a NON-BLOCKING `flock -n -x 9` probe on SLOT_FILE until the probe FAILS —
# proving some other process actually HOLDS the slot's exclusive flock — then
# returns 0.  A probe that SUCCEEDS means the slot is FREE (no holder yet), so we
# keep polling.  Returns non-zero if MAX_ITERS polls (× 0.2s) elapse without ever
# observing the slot held.
#
# WHY: replaces the fixed `sleep 0.2` holder-grace in Tests 14/15/22.  Under load
# a background holder subshell may not be scheduled within a fixed sleep, leaving
# the slot FREE when the wrapper runs → the wrapper acquires instantly (got 0 /
# elapsed 0s) instead of blocking.  Gating on the CAUSAL fact "a holder holds the
# slot" eliminates that race.  The barrier asserts a causal OUTCOME (probe fails),
# NEVER an elapsed magnitude, so it adds no wall-clock upper-bound assertion
# (respects the test_no_new_wallclock_upper_bounds guard, task 5257 / PRD W4a).
# This is the external-holder analogue of the ready-file barrier Tests 19/21A use.
#
# The MAX_ITERS×0.2s bound (default ~20s) is a BROKEN-INFRA BACKSTOP so a
# never-arriving holder cannot hang the suite — it is NOT a timing assertion.
#
# `9>>"$slot"` opens the slot file for append (creating it if absent), so a probe
# that races ahead of the holder self-heals: both converge on the same inode.
# The probe runs as an `if` condition so a non-zero `flock -n` return does not
# trip `set -euo pipefail`.
occt_wait_until_slot_held() {
    local slot="$1"
    local max_iters="${2:-100}"
    local i=0
    while [ "$i" -lt "$max_iters" ]; do
        # Probe SUCCEEDS (rc 0) ⇒ slot FREE ⇒ keep polling; FAILS (rc≠0) ⇒ a
        # holder holds the slot ⇒ confirmed.
        if ! ( flock -n -x 9 ) 9>>"$slot"; then
            return 0
        fi
        sleep 0.2
        i=$(( i + 1 ))
    done
    return 1
}
# Exported so the bounds-file negative unit test (`bash -c "! occt_wait_...")
# runs the REAL helper in the child shell (else it is a vacuous command-not-found).
export -f occt_wait_until_slot_held

# occt_plan_grep_or_dump PATTERN PLAN ERRFILE
# Plan-grep with an on-no-match child-stderr dump (task 5258, PRD
# docs/prds/merge-gate-health.md W4d tail).
#
# Greps the captured --print-plan PLAN string for the ERE PATTERN.  On a MATCH:
# returns 0 and emits NOTHING (an all-green run stays byte-for-byte unchanged).
# On NO-MATCH: echoes the captured verify.sh child-plan stderr (ERRFILE) to
# STDOUT between delimiters, then returns non-zero.
#
# WHY STDOUT: the six _T*_PLAN captures in test_occt_flock_gate.sh formerly
# redirected verify.sh stderr to /dev/null, swallowing --print-plan diagnostics
# (incl. the nextest-probe hard-fail) when a plan-string assert failed.  By
# capturing that stderr to a file and echoing it here on no-match, the existing
# assert() on-FAIL capture-dump (test_helpers.sh:42-57, esc-4959-57) surfaces it
# verbatim in the archived verify log — with ZERO changes to assert().
#
#   PATTERN  ERE fed to `grep -qE`.
#   PLAN     the multi-line captured plan string (from `--print-plan`).
#   ERRFILE  the captured verify.sh stderr (a file path; may be empty).
occt_plan_grep_or_dump() {
    local pattern="$1"
    local plan="$2"
    local errfile="$3"
    if printf '%s\n' "$plan" | grep -qE "$pattern"; then
        return 0
    fi
    echo "---- verify.sh --print-plan stderr (child plan capture) ----"
    if [ -s "$errfile" ]; then
        cat "$errfile"
    else
        echo "(child stderr was empty)"
    fi
    echo "---- end verify.sh --print-plan stderr ----"
    return 1
}
# Exported so the bounds-file negative unit test runs the real helper in its
# `bash -c` child shell (matching occt_wait_until_slot_held above).
export -f occt_plan_grep_or_dump
