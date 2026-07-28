#!/usr/bin/env bash
# tests/infra/test_warm_lane_lock_guard.sh
# Hermetic tests for scripts/warm-lane-lock-guard.sh.
#
# WHY THIS SUITE EXISTS (task 5608; escalation esc-5363-5).
#   `<worktree_base>/_merge-verify.lock` is ONE inode with THREE dark-factory
#   acquirers on THREE different waits:
#     · GitOps.merge_verify_lease            — waits 300s, then HOLDS the lock
#       for the whole verify (1-2h). On timeout it raises
#       MergeVerifyLeaseContended, which DF classifies as a REQUEUE — retryable,
#       no escalation.
#     · GitOps.reset_persistent_merge_worktree — waits only 30s
#       (_SEED_WARM_LANE_LOCK_WAIT_SECS, a hardcoded DF module constant) and on
#       timeout raises a PLAIN RuntimeError, which DF classifies as
#       category='merge_error' — TERMINAL, escalated, never requeued.
#     · _seed_warm_lane                       — `flock -x -w 30 -E 124`.
#   The defect is that ASYMMETRY on one inode: a long-held lease starves a
#   30s waiter into a spurious terminal merge_error.
#
#   Reify cannot land the behavioural fix (every decision point is DF-side).
#   Per CLAUDE.md's cross-repo invariant — "reify ships the primitive,
#   dark-factory wires the invocation" — reify ships the PRIMITIVE: a
#   read-only, non-mutating, non-blocking availability oracle for a warm-lane
#   lock, with a distinguished exit code DF can consult BEFORE dispatching into
#   its own 30s bounded wait. This suite pins that oracle's observable
#   behaviour, and nothing beyond it: no assertion here claims an end-to-end
#   merge-queue outcome, because that half is not in this task's scope.
#   Contract: docs/design/merge-verify-lane-dispatch-seam.md.
#
# run_guard captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI contract: --help, unknown flag, missing/unknown subcommand,
#       missing mount; every exit-2 path is actionable on stderr.
#   B — IDLE path and the NON-CREATING invariant: absent / unheld / shared-held
#       lock all read IDLE with empty stdout, and the probe never creates,
#       truncates, or re-inodes the lock file.
#   C — BUSY sentinel and per-lane granularity: exit 3, exactly one
#       `@@REIFY_WARM_LANE_LOCK_BUSY@@ lane=<n> lock=<p>` stdout line, and
#       --lane / REIFY_WARM_LANE_LOCK_GUARD_LANE selecting one lock, not the
#       whole mount.
#   D — FAIL-OPEN degradation: exit 3 is reachable ONLY from a positively
#       observed exclusive hold; a missing/failing flock, an unreadable lock,
#       and an absent mount all degrade to exit 0 with the sentinel ABSENT.
#   E — lock-path resolution contract (the silent-no-op guard): the lock is a
#       SIBLING of the lane dir (`<mount>/<lane>.lock`, byte-matching DF's
#       lane_lock_path()), with a decoy proving the nested misinterpretation
#       is NOT what is probed; plus the --lock-path explicit override.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-lock-guard.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-lock-guard.sh hermetic tests (task 5608) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Ambient-env hygiene
# ──────────────────────────────────────────────────────────────────────────────
# Every knob this guard reads is scrubbed from the suite's OWN environment
# before the first invocation. An ambient REIFY_WARM_LANE_MOUNT (the
# orchestrator exports one on real verify runs) would silently satisfy the
# "mount required" validation and turn Block A's missing-mount assertion into a
# vacuous pass; an ambient --lane/--lock-path knob would redirect every probe.
# Caller-side prefixes (`REIFY_..._LANE=x run_guard ...`) are unaffected: they
# are set at call time, after this scrub.
unset REIFY_WARM_LANE_MOUNT || true
unset REIFY_WARM_LANE_LOCK_GUARD_LANE || true
unset REIFY_WARM_LANE_LOCK_GUARD_LOCK_PATH || true
unset REIFY_WARM_LANE_LOCK_GUARD_FLOCK || true

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
_BGPIDS=()
cleanup() {
    for pid in "${_BGPIDS[@]+${_BGPIDS[@]}}"; do
        kill "$pid" 2>/dev/null || true
    done
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp /tmp/test-warm-lane-lock-guard-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_guard ─────────────────────────────────────────────────────────────────
# Invokes warm-lane-lock-guard.sh, capturing OUT (stdout), ERR_OUT (stderr) and
# RC (exit code) as globals — the same three-way split
# tests/infra/test_warm_lane_disk_guard.sh:88-101 uses, and the reason stdout
# assertions here can be exact: a diagnostic leaking onto stdout would show up
# in OUT rather than being swallowed into a merged stream.
# Callers may prefix inline env vars (e.g. REIFY_WARM_LANE_LOCK_GUARD_LANE=...)
# to drive the env-form contracts; those are inherited by the subshell.
run_guard() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ── run_guard_no_mount_env ────────────────────────────────────────────────────
# run_guard with REIFY_WARM_LANE_MOUNT removed from the child's environment
# outright (`env -u`), not merely unset in this shell. Belt-and-braces for the
# "no mount anywhere" assertion: it holds even if some future ambient injection
# re-exports the var after the scrub above.
run_guard_no_mount_env() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(env -u REIFY_WARM_LANE_MOUNT bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Lock-holder fixtures (technique R — causal READY-marker handshake)
#
# Copied from tests/infra/test_warm_lane_audit.sh:121-191. Required by
# docs/prds/infra-test-wallclock-deflake.md (task 4847): a fixed `sleep` races
# the background subshell's flock acquisition under load, which is exactly the
# flake class those tasks de-flaked. Nothing in this suite may assert on
# elapsed time — tests/infra/test_no_new_wallclock_upper_bounds.sh statically
# REDs that — so every assertion below is on an exit code, on stream content,
# or on file state.
# ──────────────────────────────────────────────────────────────────────────────

# _wait_for_reader_lock <ready-marker> <deadline-seconds>
# Polls for the READY marker in 0.05s ticks, returning 0 as soon as it appears
# or non-zero once the anti-hang deadline elapses. The marker is touched by a
# backgrounded holder AFTER it acquires its flock, so returning 0 causally
# guarantees the flock is held at the caller's next statement.
_wait_for_reader_lock() {
    local ready_marker="$1"
    local deadline_s="$2"
    local max_ticks=$(( deadline_s * 20 ))
    local tick=0
    while [ "$tick" -lt "$max_ticks" ]; do
        [ -f "$ready_marker" ] && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
    return 1
}

# _hold_lane_lock <mount> <lane>
# Takes an EXCLUSIVE flock on <mount>/<lane>.lock in the background — the state
# a live dark-factory consumer (merge_verify_lease / reset_persistent_merge_
# worktree / _seed_warm_lane) puts the lane in — and blocks until the READY
# marker proves the lock is held.
#
# Publishes the background pid in the GLOBAL `LANE_LOCK_PID` rather than on
# stdout deliberately: a `pid=$(_hold_lane_lock ...)` command substitution would
# run this body in a SUBSHELL, discarding the `_BGPIDS` registration below and
# orphaning the 300s sleeper past the suite's cleanup trap. Callers that need a
# per-lane handle copy LANE_LOCK_PID immediately.
LANE_LOCK_PID=""
_hold_lane_lock() {
    local mount="$1" lane="$2"
    local lock="$mount/$lane.lock"
    local ready="$lock.ready-marker"
    touch "$lock"
    ( flock -x 9 && touch "$ready" && sleep 300 ) 9>"$lock" &
    LANE_LOCK_PID=$!
    _BGPIDS+=("$LANE_LOCK_PID")
    _wait_for_reader_lock "$ready" 30
}

# _hold_lane_lock_at <lock-path>
# _hold_lane_lock's path-addressed sibling, for the Block E cases that hold a
# lock OUTSIDE the `<mount>/<lane>.lock` naming scheme (the --lock-path
# override, and the nested-path decoy). Identical technique; the only
# difference is that the caller names the inode directly.
_hold_lane_lock_at() {
    local lock="$1"
    local ready="$lock.ready-marker"
    touch "$lock"
    ( flock -x 9 && touch "$ready" && sleep 300 ) 9>"$lock" &
    LANE_LOCK_PID=$!
    _BGPIDS+=("$LANE_LOCK_PID")
    _wait_for_reader_lock "$ready" 30
}

# _hold_lane_lock_shared <mount> <lane>
# The SHARED counterpart: models a concurrent READER (e.g. another audit run),
# which the guard must read as IDLE, not BUSY (invariant A2).
#
# Two deliberate differences from _hold_lane_lock, both load-bearing:
#   · `flock -s 9`, not `-x` — the entire point of the helper.
#   · the fd is opened READ-only (`9<"$lock"`, after the `touch`), mirroring the
#     production probe's own read-only open, so the fixture models a real shared
#     reader rather than an artificial write-opened shared lock.
# The ready marker uses a DISTINCT suffix so a shared and an exclusive holder
# can never collide on one lock file's marker.
_hold_lane_lock_shared() {
    local mount="$1" lane="$2"
    local lock="$mount/$lane.lock"
    local ready="$lock.shared-ready-marker"
    touch "$lock"
    ( flock -s 9 && touch "$ready" && sleep 300 ) 9<"$lock" &
    LANE_LOCK_PID=$!
    _BGPIDS+=("$LANE_LOCK_PID")
    _wait_for_reader_lock "$ready" 30
}

# _release_lane_lock
# Kills the most recent holder and clears _BGPIDS, so the EXIT trap cannot
# double-kill a pid the kernel may already have recycled.
_release_lane_lock() {
    if [ -n "$LANE_LOCK_PID" ]; then
        kill "$LANE_LOCK_PID" 2>/dev/null || true
        wait "$LANE_LOCK_PID" 2>/dev/null || true
    fi
    LANE_LOCK_PID=""
    _BGPIDS=()
}

# ──────────────────────────────────────────────────────────────────────────────
# Blocks A-E land in subsequent commits.
# ──────────────────────────────────────────────────────────────────────────────

test_summary
