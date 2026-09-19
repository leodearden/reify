#!/usr/bin/env bash
# scripts/lib_lane_lock.sh — The SINGLE tri-state warm-lane lock probe.
#
# `<worktree_base>/<lane>.lock` is ONE inode per lane, per host, and until
# task 5738 it was probed by TWO independent implementations that disagreed
# about what a degraded measurement means. Both now call in here:
#
#   scripts/warm-lane-audit.sh       (`_probe_live`, feeding the `live=` field)
#   scripts/warm-lane-lock-guard.sh  (`_probe`, feeding the BUSY sentinel)
#
# FUNCTIONS (defined when sourced):
#   lane_lock_probe <lock_path> [<flock_bin>]
#       Takes a non-blocking SHARED flock on an EXISTING lock file and reports
#       whether an exclusive holder occupies it. flock_bin defaults to `flock`.
#       Sets LANE_LOCK_PROBE_STATE  — IDLE | BUSY | UNMEASURABLE
#       Sets LANE_LOCK_PROBE_DETAIL — reason string; EMPTY unless UNMEASURABLE
#       ALWAYS returns 0: every failure it can encounter is a degradation to
#       be reported, not an abort, and both callers run under `set -e`.
#
# The flock binary is a PARAMETER, not a knob read here. Each caller keeps its
# own documented test seam (REIFY_WARM_LANE_LOCK_GUARD_FLOCK,
# REIFY_WARM_LANE_AUDIT_FLOCK) and passes the resolved value in, so this lib
# is a pure function of (lock_path, flock_bin) with no ambient input and two
# independent CLIs stay uncoupled.
#
# INVARIANTS CARRIED HERE, because they are properties of the MEASUREMENT and
# are identical for both callers (normative statements: seam doc §3 —
# docs/design/merge-verify-lane-dispatch-seam.md):
#   A1 — NON-MUTATING. Read-only open on an EXISTING path. Never creates,
#        truncates, or changes the lock file or its parent: no `>`-open, no
#        `>>`-open, no `touch`, no `mkdir`, and deliberately NOT the
#        `flock <file> <cmd>` convenience form.
#   A2 — SHARED, non-blocking, released at once. `-s`, not `-x`: every real
#        lane consumer holds an EXCLUSIVE flock while live, so a shared request
#        still detects it, but two readers never contend with each other. A
#        point-in-time sample by nature.
#
# A3 — THE FAIL DIRECTION — IS DELIBERATELY *NOT* HERE. It belongs to the
# CONSUMER, and the two consumers are genuinely opposite: the guard fails OPEN
# (its exit 3 gates merge dispatch, where a false BUSY wedges the serial merge
# queue), the audit fails CLOSED (its output is advisory prose, where
# over-reporting occupancy is merely conservative). UNMEASURABLE exists
# precisely so ONE measurement can serve two opposite calculi — each caller
# maps it in a single `case` arm.
#
# THE -E 124 RATIONALE: `flock -n` returns a bare 1 on contention, which is
# indistinguishable from "flock itself failed" — and reading a tool fault as
# contention is a false BUSY. A distinct status separates the two;
# LANE_LOCK_PROBE_CONFLICT_RC is chosen ONLY to be DISTINGUISHABLE FROM THAT
# BARE 1, and is consumed EXCLUSIVELY within this lib — passed to `flock -E`
# below and compared against a few lines later. It matches dark-factory's
# current _SEED_WARM_LANE_LOCK_TIMEOUT_RC by CONVENTION (both echo timeout(1)'s
# 124), NOT by coupling: DF never observes this value and this lib never
# observes DF's flock rc, so a DF retune leaves this entirely correct and must
# NOT be chased here. Reasoning: seam doc §3.
#
# ACQUIRER-COUNT-AGNOSTIC: this probe detects ANY exclusive holder and encodes
# no acquirer count, cardinality or identity whatsoever. How many dark-factory
# sites acquire this inode family is seam doc §1's to state, and it has already
# moved once (four -> five); nothing here tracks it, so there is no number in
# this probe that CAN drift from a consumer's header.

# Source guard — prevent double-sourcing.
if [ "${_REIFY_LIB_LANE_LOCK_SH_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_LIB_LANE_LOCK_SH_SOURCED=1

# The would-block exit status asked of flock via -E (see the header).
LANE_LOCK_PROBE_CONFLICT_RC=124

LANE_LOCK_PROBE_STATE='IDLE'
LANE_LOCK_PROBE_DETAIL=''

lane_lock_probe() {
    local lock="$1"
    local flock_bin="${2:-flock}"
    local rc=0

    LANE_LOCK_PROBE_STATE='IDLE'
    LANE_LOCK_PROBE_DETAIL=''

    # An ABSENT lock file is not a degradation: it positively means no consumer
    # has ever taken this lane's lock. IDLE, silently, and never created (A1).
    #
    # Ordered BEFORE the flock check, and that order is load-bearing: this
    # answer needs no flock at all, so a broken or unresolvable one must not
    # turn a positive "nobody ever took this lane" into an unmeasurable one.
    # The audit probes every lane in a pool, where the difference is directly
    # observable — a degraded flock would otherwise report a lockless lane as
    # occupied.
    [ -e "$lock" ] || return 0

    # Tool missing or not executable — a wiring/environment fault, not evidence
    # about the lane.
    if ! command -v "$flock_bin" >/dev/null 2>&1; then
        LANE_LOCK_PROBE_STATE='UNMEASURABLE'
        LANE_LOCK_PROBE_DETAIL="flock is missing or not executable: $flock_bin"
        return 0
    fi

    # The read-only open is a SCOPED block redirect, deliberately NOT
    # `exec 7<"$lock" 2>/dev/null`. A redirection attached to a command-less
    # `exec` is PERMANENT for the shell, so that form would silently discard
    # every diagnostic emitted after this point, leaving an operator with no
    # way to learn the measurement had stopped working. It is safe only inside
    # a `$( )` subshell that contains the permanence — a latent constraint on
    # every call site. The block form has no such constraint and closes fd 7
    # automatically on every path.
    #
    # `2>/dev/null` is ordered BEFORE `7<"$lock"` so it is already in effect if
    # the open itself fails: redirections apply left to right, and bash reports
    # a failed one on whatever stderr is current at that moment.
    #
    # `probed` distinguishes "the body ran" from "the redirect failed" — a
    # failed redirect skips the body entirely, leaving rc at 0, which would
    # otherwise be indistinguishable from a successfully acquired lock.
    local probed=0
    {
        probed=1
        "$flock_bin" -n -s -E "$LANE_LOCK_PROBE_CONFLICT_RC" 7 || rc=$?
        if [ "$rc" -eq 0 ]; then
            # Acquired, so nobody holds it exclusively. Release at once rather
            # than relying on the fd close alone (A2).
            "$flock_bin" -u 7 || true
        fi
    } 2>/dev/null 7<"$lock" || true

    if [ "$probed" -ne 1 ]; then
        LANE_LOCK_PROBE_STATE='UNMEASURABLE'
        LANE_LOCK_PROBE_DETAIL="cannot open the lock file for reading: $lock"
        return 0
    fi

    case "$rc" in
        0)
            ;;
        "$LANE_LOCK_PROBE_CONFLICT_RC")
            # The ONLY path to BUSY: flock ran to completion and reported that
            # the shared request would block, which only an exclusive holder
            # can cause.
            LANE_LOCK_PROBE_STATE='BUSY'
            ;;
        *)
            LANE_LOCK_PROBE_STATE='UNMEASURABLE'
            LANE_LOCK_PROBE_DETAIL="flock exited $rc, which is neither acquired (0) nor would-block ($LANE_LOCK_PROBE_CONFLICT_RC)"
            ;;
    esac
    return 0
}
