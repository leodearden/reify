#!/usr/bin/env bash
# Shared serialization predicates and helpers for the OCCT flock-gate suite
# (tests/infra/test_occt_flock_gate.sh).
#
# WHY A SHARED LIB:
# Tests 20 and 21B both spawn 3 concurrent wrapper invocations with N=2 slots
# and must prove the 3rd was serialized.  Keeping the predicate here ensures the
# two tests cannot drift out of sync (one source of truth) and makes it
# unit-testable with synthetic inputs (see test_occt_flock_gate_bounds.sh),
# avoiding another timing-based test that could itself flake under load.
#
# WHY THE MILLISECOND BAND IS GONE (PRD docs/prds/infra-test-wallclock-deflake.md
# decision D1, task 6247):
# this lib used to answer "was the 3rd invocation serialized?" with an absolute
# wall-clock window, `occt_serial3_n2_within_bounds` over [LOW,HIGH]ms.  The
# ceiling was ratcheted 1200 -> 2000 -> 5000 as the merge queue grew busier
# (esc-3939-94, then task/3443's observed 3317ms) and was STILL observed at
# 5791ms.  D1 abandons absolute upper bounds for this class rather than
# re-tuning them again: the quantity being bounded is process-spawn and
# flock-acquire latency on a shared 32-core host under concurrent verify load,
# which has no defensible ceiling.
#
# The band was not merely loose, it was non-discriminating in the direction that
# mattered — this lib's own COVERAGE GAP note admitted it: three FULLY SERIAL
# invocations land ~1200ms, comfortably inside [700,5000], so an N->1
# over-serialization regression was invisible.  `occt_serial3_n2_serialized`
# replaces the band with the causal fact read straight off the slot event log,
# and detects both regressions the band could not: over-serialization (max
# concurrency 1) and a lost slot cap (max concurrency 3).
#
# The causal primitives themselves live in tests/infra/slot_holder_handshake_lib.sh,
# which is the SPOT home shared with test_test_run_semaphore.sh,
# test_lane_x_flock.sh, test_warm_lane_gc.sh and test_run_all.sh.  This lib
# SOURCES that one and callers use its `holder_*` names directly; there is no
# occt_*-prefixed forwarder for any of them.  Two such forwarders existed
# briefly and were removed: a name that only calls another name gives a reader
# two places to look and a future change two places to edit, for no gain over
# the rename.
#
# What remains here is what is genuinely OCCT-SPECIFIC — the ready-count
# barrier and gated payload this suite's multi-invocation tests synchronize on,
# and the exactly-2 specialization of the shared max-concurrency predicate.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_OCCT_FLOCK_GATE_LIB_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_OCCT_FLOCK_GATE_LIB_SH_SOURCED=1

_reify_occt_lib_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[ -f "$_reify_occt_lib_dir/slot_holder_handshake_lib.sh" ] || {
    echo "ERROR: slot_holder_handshake_lib.sh not found at $_reify_occt_lib_dir/slot_holder_handshake_lib.sh" >&2
    return 1 2>/dev/null || exit 1
}
# shellcheck disable=SC1090,SC1091
source "$_reify_occt_lib_dir/slot_holder_handshake_lib.sh"

# plan_capture_lib.sh supplies plan_match, the fork-free matcher
# occt_plan_grep_or_dump below is built on. It carries its own source guard, so
# a test that already sourced it pays nothing for sourcing it again here.
[ -f "$_reify_occt_lib_dir/plan_capture_lib.sh" ] || {
    echo "ERROR: plan_capture_lib.sh not found at $_reify_occt_lib_dir/plan_capture_lib.sh" >&2
    return 1 2>/dev/null || exit 1
}
# shellcheck disable=SC1090,SC1091
source "$_reify_occt_lib_dir/plan_capture_lib.sh"
unset _reify_occt_lib_dir

# occt_wait_for_ready_count BARRIER_DIR N [BASE_ITERS=100]
# Return 0 once BARRIER_DIR holds at least N `ready-*` files — the multi-payload
# form of the ready-file handshake, used by Tests 20, 21A and 21B where several
# wrapper invocations each announce that they now hold a slot.
#
# Waiting for a COUNT rather than a named marker is what lets a test with N
# slots and more than N invocations synchronize at all: waiting for every
# payload would deadlock, since the surplus invocations cannot enter their
# critical section until an earlier one leaves.
#
# BASE_ITERS x 0.2s (load-scaled) is a BROKEN-INFRA BACKSTOP so a payload that
# never arrives cannot hang the suite — it is NOT a timing assertion.  Callers
# assert on the resulting event log, so an exhausted budget yields a clean RED
# rather than a hang.
occt_wait_for_ready_count() {
    local _dir="$1" _want="$2"
    local _budget
    _budget="$(load_tolerant_attempts "${3:-100}")"
    local _i=0
    while [ "$_i" -lt "$_budget" ]; do
        if [ "$(find "$_dir" -maxdepth 1 -name 'ready-*' | wc -l)" -ge "$_want" ]; then
            return 0
        fi
        sleep 0.2
        _i=$(( _i + 1 ))
    done
    return 1
}
export -f occt_wait_for_ready_count

# occt_hold_until_go [BASE_ITERS=300]
# THE gated payload every barrier-synchronized wrapper invocation in this suite
# runs, in one place instead of pasted into each `bash -c` body.
#
# Announce "I now hold a slot" by touching ready-$$ in OCCT_BARRIER_DIR, then
# keep holding until the TEST touches `go`.  Pinning the holders that way is
# what MAKES the contention happen rather than hoping the spawns overlap, and it
# is the payload half of the handshake whose waiting half is
# occt_wait_for_ready_count above.
#
# OCCT_BARRIER_DIR arrives through the ENVIRONMENT (a `VAR=... "$WRAPPER"`
# prefix at the call site), not by interpolating the path into a quoted script
# body.  The body is then a single identifier, so the ten call sites cannot
# drift from one another the way ten pasted copies of the loop could.
#
# BASE_ITERS x 0.2s (load-scaled) is a BROKEN-INFRA BACKSTOP so a test that dies
# before touching `go` cannot leave a payload holding a slot forever -- it is
# NOT a timing assertion.  The base is deliberately LARGER than
# occt_wait_for_ready_count's, because the hold must outlive the barrier that
# waits on it: were they equal, a slow-but-healthy run could have the hold
# expire at the same moment the barrier gave up, and both scale by the same
# factor so the ordering holds at every load level.
occt_hold_until_go() {
    local _dir="${OCCT_BARRIER_DIR:?occt_hold_until_go: OCCT_BARRIER_DIR must be set by the caller}"
    local _budget
    _budget="$(load_tolerant_attempts "${1:-300}")"
    touch "$_dir/ready-$$"
    local _i=0
    while [ ! -e "$_dir/go" ] && [ "$_i" -lt "$_budget" ]; do
        sleep 0.2
        _i=$(( _i + 1 ))
    done
}
# Exported so it survives the `"$WRAPPER" bash -c` hop at every call site (and
# so the bounds-file unit cases run the REAL helper in their child shell).
export -f occt_hold_until_go

# occt_serial3_n2_serialized EVENT_LOG
# Returns 0 iff the log shows a maximum of EXACTLY 2 slots held at once — the
# causal signature of three invocations correctly serialized behind a 2-slot
# cap.  Replaces the retired [700,5000]ms band (see the header): 1 means the
# gate over-serialized to N=1, 3 means the cap was lost, 0 means no wrapper
# ever recorded an acquire, and all four cases are now distinguishable.
occt_serial3_n2_serialized() {
    [ "$(holder_max_concurrent "$1")" -eq 2 ]
}
# Exported so the bounds-file negative unit tests (`bash -c "! occt_serial3..."`)
# run the REAL helper in the child shell rather than a vacuous
# command-not-found.
export -f occt_serial3_n2_serialized

# occt_plan_grep_or_dump PATTERN PLAN ERRFILE
# Plan-grep with an on-no-match child-stderr dump (task 5258, PRD
# docs/prds/merge-gate-health.md W4d tail).
#
# Matches the captured --print-plan PLAN string against the ERE PATTERN.  On a
# MATCH: returns 0 and emits NOTHING (an all-green run stays byte-for-byte
# unchanged).  On NO-MATCH: echoes the PATTERN, the PLAN and the captured
# verify.sh child-plan stderr (ERRFILE) to STDOUT between delimiters, then
# returns non-zero.
#
# FORK-FREE MATCHING, via plan_match rather than `printf | grep -qE`.  A
# pipe-to-grep match is not a pure predicate on the plan string: grep -q exits
# on its FIRST match and closes the pipe, so once the plan outgrows the 64 KiB
# pipe buffer the printf still writing on the other end takes SIGPIPE, and
# under a caller's `set -o pipefail` the pipeline reports 141 for a pattern
# that is PRESENT.  plan_match preserves grep -qE's per-line REG_NEWLINE
# semantics exactly — which is what keeps the existing T-series patterns
# (alternation, same-line `.*`, escaped dot/star) matching identically — while
# carrying neither the pipe nor the fork.  This was the last live instance of
# the idiom documented as the esc-4574-42 spurious-FAIL class in
# plan_capture_lib.sh's own header.
#
# WHY THE PATTERN AND THE PLAN ARE DUMPED, not just ERRFILE: verify.sh writes
# EMPTY stderr on a healthy --print-plan, so a failing plan assertion used to
# archive the literal "(child stderr was empty)" and nothing whatsoever about
# what was searched or what was there.  That is what made this class of failure
# arrive unattributable.  The plan is bounded so an oversize capture cannot
# swamp the archived verify log, and says so when it truncates rather than
# trailing off silently.
#
# WHY STDOUT: the six _T*_PLAN captures in test_occt_flock_gate.sh formerly
# redirected verify.sh stderr to /dev/null, swallowing --print-plan diagnostics
# (incl. the nextest-probe hard-fail) when a plan-string assert failed.  By
# capturing that stderr to a file and echoing it here on no-match, the existing
# assert() on-FAIL capture-dump (test_helpers.sh:42-57, esc-4959-57) surfaces it
# verbatim in the archived verify log — with ZERO changes to assert().
#
#   PATTERN  ERE, matched per-line (grep -qE semantics) by plan_match.
#   PLAN     the multi-line captured plan string (from `--print-plan`).
#   ERRFILE  the captured verify.sh stderr (a file path; may be empty).
_OCCT_PLAN_DUMP_MAX_LINES="${_OCCT_PLAN_DUMP_MAX_LINES:-120}"
occt_plan_grep_or_dump() {
    local pattern="$1"
    local plan="$2"
    local errfile="$3"
    if plan_match "$plan" "$pattern"; then
        return 0
    fi
    echo "---- plan pattern that did NOT match ----"
    echo "$pattern"
    echo "---- verify.sh --print-plan plan (searched) ----"
    local _n=0 _line
    while IFS= read -r _line; do
        _n=$((_n + 1))
        if [ "$_n" -gt "$_OCCT_PLAN_DUMP_MAX_LINES" ]; then
            echo "(plan dump truncated at $_OCCT_PLAN_DUMP_MAX_LINES lines)"
            break
        fi
        echo "$_line"
    done <<< "$plan"
    echo "---- end verify.sh --print-plan plan ----"
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
# `bash -c` child shell (matching occt_serial3_n2_serialized above).
export -f occt_plan_grep_or_dump
