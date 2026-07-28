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

# _flock_holder_body <flock-mode> <ready-marker> <release-marker>
# Body of a RELEASE-marker flock holder. Backgrounded by the helpers below as
# `( _flock_holder_body ... ) 9>lock &` — fd 9 is supplied by the CALLER's
# redirect, so the flock is taken by that backgrounded subshell directly, and it
# is a DIRECT child of this script, so a later `wait $!` on it works.
#
# NOT `sleep N` + `kill`, which is the obvious shape and is WRONG here: the
# `sleep` child inherits fd 9, so killing only the parent subshell can leave fd
# 9 — and therefore the flock — held after the "release", flaking the NEXT
# block's IDLE assertion into a spurious BUSY. This suite releases and re-takes
# locks repeatedly (B→C→D→E), so it needs the causal form:
# `touch RELEASE; wait $pid` guarantees the flock is dropped before the caller's
# next statement. Technique borrowed from
# tests/infra/test_warm_lane_sizing_lifecycle.sh:129-157.
#
# The tick ceiling is an anti-hang safeguard, not an assertion — nothing here
# asserts on elapsed time (docs/prds/infra-test-wallclock-deflake.md DD5).
_flock_holder_body() {
    local mode="$1" ready="$2" release="$3"
    flock "$mode" 9
    touch "$ready"
    local tick=0
    while [ "$tick" -lt 6000 ]; do
        [ -f "$release" ] && return 0
        sleep 0.05
        tick=$(( tick + 1 ))
    done
}

# _hold_lane_lock_at <lock-path> [flock-mode]
# Takes a flock (default EXCLUSIVE) on an explicitly-named lock file — the state
# a live dark-factory consumer (merge_verify_lease / reset_persistent_merge_
# worktree / _seed_warm_lane) puts a lane in — and blocks until the READY marker
# proves the lock is held. Path-addressed so Block E can hold a lock OUTSIDE the
# `<mount>/<lane>.lock` naming scheme (the --lock-path override, and the decoy).
#
# Publishes the background pid in the GLOBAL `LANE_LOCK_PID` rather than on
# stdout deliberately: a `pid=$(_hold_lane_lock_at ...)` command substitution
# would run this body in a SUBSHELL, discarding the `_BGPIDS` registration below
# and orphaning the holder past the suite's cleanup trap.
#
# ONE HOLDER AT A TIME is this suite's standing invariant: _release_lane_lock
# clears _BGPIDS wholesale, so a second concurrent holder would be dropped from
# the cleanup set. Every block below holds one lock, asserts, and releases.
#
# The exclusive holder opens fd 9 for WRITING (`9>`); the shared one opens it
# READ-only (`9<`), mirroring the production probe's own read-only open, so the
# shared fixture models a real reader rather than an artificial write-opened
# shared lock. Ready/release markers are suffixed per mode so an exclusive and a
# shared holder can never collide on one lock file's markers.
LANE_LOCK_PID=""
LANE_LOCK_RELEASE=""
_hold_lane_lock_at() {
    local lock="$1" mode="${2:--x}"
    local tag="exclusive"
    [ "$mode" = "-s" ] && tag="shared"
    local ready="$lock.$tag-ready-marker"
    local release="$lock.$tag-release-marker"
    rm -f "$ready" "$release"
    touch "$lock"
    if [ "$mode" = "-s" ]; then
        ( _flock_holder_body "$mode" "$ready" "$release" ) 9<"$lock" &
    else
        ( _flock_holder_body "$mode" "$ready" "$release" ) 9>"$lock" &
    fi
    LANE_LOCK_PID=$!
    LANE_LOCK_RELEASE="$release"
    _BGPIDS+=("$LANE_LOCK_PID")
    _wait_for_reader_lock "$ready" 30
}

# _hold_lane_lock <mount> <lane> — the lane-addressed exclusive form.
_hold_lane_lock() {
    local mount="$1" lane="$2"
    _hold_lane_lock_at "$mount/$lane.lock" -x
}

# _hold_lane_lock_shared <mount> <lane>
# The SHARED counterpart: models a concurrent READER (e.g. another audit run),
# which the guard must read as IDLE, not BUSY — `flock -s`, not `-x`, is the
# entire point of the helper.
_hold_lane_lock_shared() {
    local mount="$1" lane="$2"
    _hold_lane_lock_at "$mount/$lane.lock" -s
}

# _release_lane_lock
# Drops the current holder CAUSALLY: touch its release marker, then `wait` for
# the subshell to exit, which closes fd 9 and releases the flock before the
# caller's next statement. Clearing _BGPIDS afterwards prevents the EXIT trap
# from signalling a pid the kernel may already have recycled.
_release_lane_lock() {
    [ -n "$LANE_LOCK_PID" ] || return 0
    touch "$LANE_LOCK_RELEASE"
    wait "$LANE_LOCK_PID" 2>/dev/null || true
    LANE_LOCK_PID=""
    LANE_LOCK_RELEASE=""
    _BGPIDS=()
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI contract
#
# Exit 2 is the WIRING-BUG code, deliberately distinct from both verdicts (0
# IDLE / 3 BUSY): dark-factory must be able to tell "I asked the question wrong"
# from "the lane is free". A guard that answered IDLE to a malformed invocation
# would let a wiring mistake read as a green light forever.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI contract ---"

# A1: --help is a successful, self-documenting exit, and it keeps stdout clean —
# stdout is reserved for the BUSY sentinel alone, so a caller that parses stdout
# never has to filter help text out of it.
run_guard --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints the usage banner on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -q "Usage:"' _ "$ERR_OUT"
assert "A1: --help writes nothing to stdout (stdout is sentinel-only)" \
    test -z "$OUT"

# A2: an unknown flag is a wiring bug, not a verdict.
run_guard --bogus
assert "A2: unknown flag exits 2" test "$RC" -eq 2
assert "A2: unknown flag explains itself on stderr" test -n "$ERR_OUT"

# A3: a bare invocation with no subcommand at all.
run_guard
assert "A3: missing subcommand exits 2" test "$RC" -eq 2
assert "A3: missing subcommand explains itself on stderr" test -n "$ERR_OUT"

# A4: an unknown subcommand — distinct from A2's unknown FLAG, because the arg
# loop reaches it through the bare-word branch rather than the -* branch.
run_guard frobnicate
assert "A4: unknown subcommand exits 2" test "$RC" -eq 2
assert "A4: unknown subcommand explains itself on stderr" test -n "$ERR_OUT"

# A5: `check` with no mount anywhere. Invoked through run_guard_no_mount_env so
# an ambient REIFY_WARM_LANE_MOUNT (the orchestrator exports one on real verify
# runs) cannot mask the missing-mount validation and turn this into a vacuous
# pass.
run_guard_no_mount_env check
assert "A5: check with neither --mount nor REIFY_WARM_LANE_MOUNT exits 2" \
    test "$RC" -eq 2
assert "A5: the missing-mount error names the knob on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -q "REIFY_WARM_LANE_MOUNT"' _ "$ERR_OUT"

# A6: no exit-2 path writes to stdout. Same reason as A1 — a caller reading
# stdout for the sentinel must never see usage-error text there.
run_guard --bogus
assert "A6: an exit-2 path writes nothing to stdout" test -z "$OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block B — IDLE path and the NON-CREATING invariant
#
# The probe is a pure reader of pool state. A guard that materialized the very
# lock file it was asked about would be creating the resource dark-factory
# serializes on — so B2/B5 pin non-creation and non-mutation as hard as B1/B3/B4
# pin the verdicts. Both would be satisfied by a `>`-open or the
# `flock <file> <cmd>` convenience form failing loudly, which is the point.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: IDLE path and the non-creating invariant ---"

B_MOUNT="$(mktemp -d /tmp/test-warm-lane-lock-guard-b-XXXXXX)"
_TMPDIRS+=("$B_MOUNT")
mkdir -p "$B_MOUNT/_merge-verify"
B_LOCK="$B_MOUNT/_merge-verify.lock"

# B1: no lock file at all — the state a lane sits in until DF first seeds it.
run_guard check --mount "$B_MOUNT"
assert "B1: an absent lock file reads IDLE (exit 0)" test "$RC" -eq 0
assert "B1: IDLE writes nothing to stdout" test -z "$OUT"

# B2: ...and the probe did not bring it into existence.
assert "B2: the probe did NOT create the absent lock file" test ! -e "$B_LOCK"

# B3: present but unheld — the steady state of a free lane.
touch "$B_LOCK"
run_guard check --mount "$B_MOUNT"
assert "B3: a present-but-unheld lock reads IDLE (exit 0)" test "$RC" -eq 0
assert "B3: a present-but-unheld lock writes nothing to stdout" test -z "$OUT"

# B5a: probing an unheld lock leaves the inode byte-identical. Size AND inode
# together: a truncating open would change the size, and a create-and-replace
# would change the inode while leaving the size at 0.
B_STAT_BEFORE="$(stat -c '%s %i' "$B_LOCK")"
run_guard check --mount "$B_MOUNT"
B_STAT_AFTER="$(stat -c '%s %i' "$B_LOCK")"
assert "B5a: probing an unheld lock leaves its size and inode unchanged" \
    test "$B_STAT_BEFORE" = "$B_STAT_AFTER"

# B4: a concurrent SHARED reader (e.g. another audit run) must NOT read BUSY.
# The probe itself takes a shared lock, so only a genuine exclusive holder can
# block it — that is what makes concurrent oracles non-contending.
_hold_lane_lock_shared "$B_MOUNT" _merge-verify
run_guard check --mount "$B_MOUNT"
assert "B4: a concurrent SHARED reader still reads IDLE (exit 0)" test "$RC" -eq 0
assert "B4: a concurrent SHARED reader writes nothing to stdout" test -z "$OUT"

# B5b: ...and probing under a shared holder is equally non-mutating.
B_STAT_SHARED="$(stat -c '%s %i' "$B_LOCK")"
assert "B5b: probing a shared-held lock leaves its size and inode unchanged" \
    test "$B_STAT_BEFORE" = "$B_STAT_SHARED"
_release_lane_lock

# B6: the load-bearing RED for this block. With a genuine EXCLUSIVE holder the
# guard must not report IDLE — asserted as "not 0" here rather than "is 3",
# because the exact code and its sentinel are Block C's contract.
_hold_lane_lock "$B_MOUNT" _merge-verify
run_guard check --mount "$B_MOUNT"
assert "B6: an EXCLUSIVE holder must NOT read IDLE (exit != 0)" test "$RC" -ne 0
_release_lane_lock

# ──────────────────────────────────────────────────────────────────────────────
# Block C — the BUSY sentinel and per-lane granularity
#
# This is the machine-readable half of the cross-repo seam. Exit 3 is the pinned
# throttle-not-requeue code (docs/prds/warm-lane-pool-sizing-lifecycle.md
# §212-219; the same code warm-lane-disk-guard.sh --soft and
# fleet-load-detector.sh emit), and stdout carries exactly one sentinel line so
# dark-factory can read the verdict without parsing prose.
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: BUSY sentinel and per-lane granularity ---"

C_MOUNT="$(mktemp -d /tmp/test-warm-lane-lock-guard-c-XXXXXX)"
_TMPDIRS+=("$C_MOUNT")
mkdir -p "$C_MOUNT/_merge-verify" "$C_MOUNT/_spec-0"

# ── Phase 1: an exclusive holder on _merge-verify ─────────────────────────────
_hold_lane_lock "$C_MOUNT" _merge-verify
run_guard check --mount "$C_MOUNT"

# C1: exactly 3, and specifically NOT the neighbouring codes. 3 means "defer this
# dispatch"; 75 (EX_TEMPFAIL) would tell DF to REQUEUE the command, and 2 means
# "you wired me wrong" — asserting the two negatives keeps 3 a deliberate
# cross-repo value rather than an incidental non-zero.
assert "C1: an exclusive holder exits exactly 3" test "$RC" -eq 3
assert "C1: BUSY is not 75 (EX_TEMPFAIL requeue), a different instruction" test "$RC" -ne 75
assert "C1: BUSY is not 2 (usage error), a different meaning" test "$RC" -ne 2

# C2: stdout is exactly one line, and that line is the sentinel. The line-count
# assertion is what keeps a diagnostic from leaking onto stdout alongside it.
assert "C2: BUSY writes a non-empty stdout" test -n "$OUT"
assert "C2: BUSY stdout is exactly one line" \
    test "$(printf '%s\n' "$OUT" | wc -l)" -eq 1
assert "C2: that line is the @@REIFY_WARM_LANE_LOCK_BUSY@@ sentinel with lane= and lock=" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^@@REIFY_WARM_LANE_LOCK_BUSY@@ lane=_merge-verify lock=.*/_merge-verify\.lock$"' \
    _ "$OUT"

# C3: a human reading the log gets an actionable message naming the lane.
assert "C3: BUSY explains itself on stderr" test -n "$ERR_OUT"
assert "C3: the stderr message names the lane" \
    bash -c 'printf "%s\n" "$1" | grep -qF "_merge-verify"' _ "$ERR_OUT"

# C4: per-lane granularity. A guard that probed the whole mount — or any lock it
# found under it — would call _spec-0 busy here purely because a NEIGHBOUR is.
run_guard check --mount "$C_MOUNT" --lane _spec-0
assert "C4: a busy neighbour does not make _spec-0 busy (exit 0)" test "$RC" -eq 0
assert "C4: the unaffected lane writes nothing to stdout" test -z "$OUT"
_release_lane_lock

# ── Phase 2: move the holder to _spec-0 ────────────────────────────────────────
_hold_lane_lock "$C_MOUNT" _spec-0

# C5: --lane is honoured POSITIVELY, not just negatively — C4 alone would also
# pass for a guard that always answered IDLE to a --lane it did not understand.
run_guard check --mount "$C_MOUNT" --lane _spec-0
assert "C5: --lane _spec-0 with a holder on _spec-0 exits 3" test "$RC" -eq 3
assert "C5: the sentinel reports lane=_spec-0" \
    bash -c 'printf "%s\n" "$1" | grep -qE "^@@REIFY_WARM_LANE_LOCK_BUSY@@ lane=_spec-0 lock=.*/_spec-0\.lock$"' \
    _ "$OUT"

# C6: the env form is equivalent to the flag...
REIFY_WARM_LANE_LOCK_GUARD_LANE=_spec-0 run_guard check --mount "$C_MOUNT"
assert "C6: REIFY_WARM_LANE_LOCK_GUARD_LANE=_spec-0 is equivalent to --lane _spec-0" \
    test "$RC" -eq 3
assert "C6: the env form produces the same lane= field" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_spec-0"' _ "$OUT"

# ...and the flag WINS over it (standard house ordering: env supplies the
# default, the flag overwrites it). _merge-verify is unheld in this phase, so a
# guard that let the env value win would answer 3 instead of 0.
REIFY_WARM_LANE_LOCK_GUARD_LANE=_spec-0 run_guard check --mount "$C_MOUNT" --lane _merge-verify
assert "C6: an explicit --lane overrides REIFY_WARM_LANE_LOCK_GUARD_LANE" test "$RC" -eq 0
assert "C6: the overridden (idle) lane writes nothing to stdout" test -z "$OUT"
_release_lane_lock

test_summary
