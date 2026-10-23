#!/usr/bin/env bash
# tests/infra/run-all-member-budget-lib.sh — run one command under a
# wall-clock budget in a process group of its own, and say whether it hit
# that budget (task 9441).
#
# Sourced by tests/infra/run_all.sh, which runs every member through
# member_budget_run and attributes a timeout with member_budget_exceeded.
# The knobs, their defaults and their validation belong to run_all.sh: budget
# and grace arrive here as arguments, and nothing here reads a caller global.
#
# MECHANISM. GNU `timeout --kill-after=GRACE BUDGET cmd` makes itself the
# leader of a new process group and runs cmd in it. When BUDGET expires it
# TERMs the whole group and exits 124. If anything in the group is still alive
# GRACE seconds later, it KILLs the group, itself included, so its status is
# 137. A member's background children therefore die with it, and so does a
# member that ignores TERM.
#
# THE RELAY. A process in a group of its own no longer receives a signal sent
# to its caller's group. The merge envelope TERMs run_all.sh's whole group, so
# that TERM would no longer reach the member. member_budget_run therefore runs
# as a subshell that stays in the caller's group. It traps TERM, INT and HUP and
# forwards TERM to the timeout process. timeout then TERMs the member's group
# and, after GRACE, KILLs it, whether or not the caller is still alive.
#
# RESIDUAL. A GNU timeout nested inside the command makes a group of its own,
# which the group kill does not reach. It stays bounded by its own duration.
#
# Imposes no shell option on its caller and never exits the caller's shell:
# member_budget_run is a subshell, and every command in it that can fail is
# guarded, so it behaves the same with or without the caller's set -e.

# Source guard: sourcing twice is a no-op.
if [ "${_REIFY_RUN_ALL_MEMBER_BUDGET_LIB_SOURCED:-}" = "1" ]; then
    return 0 2>/dev/null || true
fi
_REIFY_RUN_ALL_MEMBER_BUDGET_LIB_SOURCED=1

# member_budget_run <budget-s> <grace-s> <cmd> [arg...]
#   Runs <cmd> under `timeout --kill-after=<grace-s>s <budget-s>s` with stdin
#   from /dev/null, and exits with timeout's status. That is <cmd>'s own status
#   when it finishes in time, 124 when the budget expired and the TERM sufficed,
#   and 137 when the group had to be KILLed.
#   A TERM, INT or HUP delivered to this subshell is forwarded to timeout as
#   TERM, and the subshell then waits for timeout to finish.
member_budget_run() (
    _mb_budget="$1" _mb_grace="$2"
    shift 2
    _mb_pid="" _mb_relayed=0
    trap '_mb_relayed=1; [ -z "$_mb_pid" ] || kill -TERM "$_mb_pid" 2>/dev/null || true' TERM INT HUP
    # Async, so that $! is the timeout process itself, the leader of the
    # member's group.
    ( exec timeout --kill-after="${_mb_grace}s" "${_mb_budget}s" "$@" ) </dev/null &
    _mb_pid=$!
    # A signal that arrived before $! was known has not been forwarded yet.
    [ "$_mb_relayed" = 0 ] || kill -TERM "$_mb_pid" 2>/dev/null || true
    while :; do
        _mb_relayed=0 _mb_rc=0
        wait "$_mb_pid" 2>/dev/null || _mb_rc=$?
        # A forwarded signal can end the wait early, with 128+n, before
        # timeout's own status is in. Wait again: bash keeps a child's status
        # after reaping it, and after a wait has returned it, so this wait
        # returns timeout's status whether timeout is still tearing its group
        # down, has finished since, or had finished before the signal arrived.
        [ "$_mb_relayed" = 1 ] || break
    done
    exit "$_mb_rc"
)

# member_budget_exceeded <rc> <elapsed-ms> <budget-s>
#   The one definition of "this run hit its budget": member_budget_run's status
#   is a timeout status (124 TERMed, 137 KILLed), AND the run lasted at least
#   the budget. A command that exits 124 or 137 on its own, before its budget,
#   is an ordinary failure. An elapsed time that is not an integer number of
#   milliseconds never counts as a budget expiry. Both numbers are decimal,
#   leading zeros included, as GNU timeout reads the budget.
member_budget_exceeded() {
    case "$1" in
        124|137) ;;
        *) return 1 ;;
    esac
    [[ "$2" =~ ^[0-9]+$ ]] || return 1
    [[ "$3" =~ ^[0-9]+$ ]] || return 1
    [ "$2" -ge $(( 10#$3 * 1000 )) ]
}
