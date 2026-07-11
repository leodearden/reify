#!/usr/bin/env bash
# tests/infra/test_warm_lane_audit.sh
# Hermetic tests for scripts/warm-lane-audit.sh.
#
# PRD: docs/prds/warm-lane-pool-sizing-lifecycle.md §9.1 (task α); boundary
# tests B1/B2 (§10). Reuses the task-4749 status-oracle seam
# (REIFY_LANE_LEAK_STATUS_CMD) and the <lane_dir>.lock flock convention from
# scripts/warm-lane-gc.sh / tests/infra/test_warm_lane_gc.sh.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# Blocks:
#   A — CLI guard: --help, unknown flag
#   (additional blocks land in subsequent commits as the script grows: the
#    ASSIGNED/FREE probe + LIVE + headroom, backing-task-status +
#    RECLAIMABLE-via-terminal, recoverable LANDED/PUSHED/ORPHAN,
#    residue-only-dirty, LEAKED + stale, measured disk figures, --format
#    json, the combined B1 fixture + A3 degradation, and B2 read-only.)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-audit.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-audit.sh hermetic tests (task 5172) ==="

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

ERR_FILE="$(mktemp /tmp/test-warm-lane-audit-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ─────────────────────────────────────────────────────────────────
# Invokes warm-lane-audit.sh, capturing OUT (stdout), ERR_OUT (stderr), RC.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLI guard
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: CLI guard ---"

# A1: --help exits 0 and prints usage on stderr
run_helper --help
assert "A1: --help exits 0" test "$RC" -eq 0
assert "A1: --help prints 'usage' or 'Usage' on stderr" \
    bash -c 'printf "%s\n" "$1" | grep -qi "usage"' _ "$ERR_OUT"

# A2: unknown flag exits 2
run_helper --nope
assert "A2: unknown flag exits 2" test "$RC" -eq 2

# ──────────────────────────────────────────────────────────────────────────────
# Shared fixture scaffolding: make_lane + _wait_for_reader_lock
# ──────────────────────────────────────────────────────────────────────────────

# make_lane DIR [BRANCH]
# Creates a minimal standalone git repo at DIR (one initial commit on main).
# BRANCH: task/NNNN -> checkout a new branch; DETACH -> detach HEAD;
# main/"" -> stay on main. Mirrors tests/infra/test_warm_lane_preflight.sh's
# identically-named helper (Block D) — each lane is its OWN independent repo
# (not a linked worktree of a shared primary), which is sufficient for
# warm-lane-audit.sh's per-lane git predicates (merge-base/status/symbolic-ref
# all operate within a single lane's own repo).
make_lane() {
    local dir="$1" branch="${2:-}"
    git init -q -b main "$dir"
    git -C "$dir" config user.email "test@test.local"
    git -C "$dir" config user.name "Test"
    touch "$dir/README.md"
    git -C "$dir" add README.md
    git -C "$dir" commit -q -m "initial"
    case "$branch" in
        task/*)
            git -C "$dir" checkout -q -b "$branch" ;;
        DETACH)
            git -C "$dir" checkout -q --detach ;;
        main|"")
            : ;;
    esac
}

# _wait_for_reader_lock <ready-marker> <deadline-seconds>
# Causal ordering (technique R, docs/prds/infra-test-wallclock-deflake.md,
# task #4847): polls for the READY marker file in 0.05s ticks, returning 0 as
# soon as it appears, or non-zero once the anti-hang deadline elapses. The
# READY marker is touched by a backgrounded lock holder AFTER it acquires its
# flock, so returning 0 causally guarantees the flock is held at the caller's
# next statement -- replacing a fixed `sleep` that races the background
# subshell's lock acquisition under load. Mirrors tests/infra/test_warm_lane_gc.sh's
# identically-named helper.
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

# ──────────────────────────────────────────────────────────────────────────────
# Block B — ASSIGNED/FREE probe + LIVE + headroom counts
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: ASSIGNED/FREE probe + LIVE + headroom counts ---"

B_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-b-XXXXXX)"
_TMPDIRS+=("$B_MOUNT")

# _lane-live: a live consumer holds the lane's exclusive flock (ASSIGNED -> LIVE).
# Causal READY-marker handshake instead of a fixed sleep (de-flake convention,
# tests/infra/test_warm_lane_gc.sh Block I6 precedent).
make_lane "$B_MOUNT/_lane-live"
touch "$B_MOUNT/_lane-live.lock"
B_READY="$B_MOUNT/_lane-live.lock.ready-marker"
( flock -x 9 && touch "$B_READY" && sleep 300 ) 9>"$B_MOUNT/_lane-live.lock" &
B_LOCK_PID=$!
_BGPIDS+=("$B_LOCK_PID")
_wait_for_reader_lock "$B_READY" 30

# _lane-free: unheld, pre-created lock file (FREE).
make_lane "$B_MOUNT/_lane-free"
touch "$B_MOUNT/_lane-free.lock"

run_helper --mount "$B_MOUNT"

assert "B1: exit 0" test "$RC" -eq 0
assert "B2: _lane-live row shows ASSIGNED" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-live .*assigned=ASSIGNED"' _ "$OUT"
assert "B3: _lane-live classification LIVE" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-live .*classification=LIVE"' _ "$OUT"
assert "B4: _lane-free row shows FREE" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-free .*assigned=FREE"' _ "$OUT"
assert "B5: HEADROOM line shows resident=2" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "resident=2"' _ "$OUT"
assert "B6: HEADROOM line shows assigned=1" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "assigned=1"' _ "$OUT"
assert "B7: HEADROOM line shows free=1" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "free=1"' _ "$OUT"

# Release the lock
kill "$B_LOCK_PID" 2>/dev/null || true
_BGPIDS=()  # clear so cleanup doesn't double-kill

# ──────────────────────────────────────────────────────────────────────────────
# Shared scaffolding: status-oracle stub + ahead-of-main commit helper
# ──────────────────────────────────────────────────────────────────────────────

# _add_ahead_commit DIR — add a committed change NOT reachable from main.
_add_ahead_commit() {
    local dir="$1"
    echo "ahead" >> "$dir/README.md"
    git -C "$dir" add README.md
    git -C "$dir" commit -q -m "ahead-of-main commit"
}

# leak-oracle.sh: given task-id $1, looks up status in ORACLE_MAP file (one
# "id status" pair per line). Exits 0 with empty output for unknown ids.
# Mirrors tests/infra/test_warm_lane_preflight.sh Block D / test_warm_lane_gc.sh
# byte-for-byte (the shared task-4749 status-oracle contract).
ORACLE_STUB_DIR="$(mktemp -d /tmp/test-warm-lane-audit-oracle-stub-XXXXXX)"
_TMPDIRS+=("$ORACLE_STUB_DIR")
cat > "$ORACLE_STUB_DIR/leak-oracle.sh" << 'STUB_EOF'
#!/usr/bin/env bash
_qid="$1"
if [ -f "${ORACLE_MAP:-}" ]; then
    while IFS=' ' read -r _mid _mst; do
        if [ "$_mid" = "$_qid" ]; then
            printf '%s\n' "$_mst"
            exit 0
        fi
    done < "$ORACLE_MAP"
fi
exit 0
STUB_EOF
chmod +x "$ORACLE_STUB_DIR/leak-oracle.sh"

# leak-oracle-fail.sh: always exits non-zero -- drives the A3 hardening test
# (oracle failure must NOT abort; unknown = neither terminal nor non-terminal).
cat > "$ORACLE_STUB_DIR/leak-oracle-fail.sh" << 'STUB_EOF'
#!/usr/bin/env bash
exit 1
STUB_EOF
chmod +x "$ORACLE_STUB_DIR/leak-oracle-fail.sh"

# ──────────────────────────────────────────────────────────────────────────────
# Block C — backing-task-status via the 4749 seam + terminal -> RECLAIMABLE
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: backing-task-status + terminal -> RECLAIMABLE ---"

C_MOUNT="$(mktemp -d /tmp/test-warm-lane-audit-c-XXXXXX)"
_TMPDIRS+=("$C_MOUNT")

C_MAP="$(mktemp /tmp/test-warm-lane-audit-c-map-XXXXXX)"
_TMPDIRS+=("$C_MAP")
printf '100 done\n200 pending\n' > "$C_MAP"

# _lane-done: FREE, task/100, oracle=done, ahead-of-main.
make_lane "$C_MOUNT/_lane-done" "task/100"
_add_ahead_commit "$C_MOUNT/_lane-done"

# _lane-pending: FREE, task/200, oracle=pending, ahead-of-main.
make_lane "$C_MOUNT/_lane-pending" "task/200"
_add_ahead_commit "$C_MOUNT/_lane-pending"

ORACLE_MAP="$C_MAP" run_helper --mount "$C_MOUNT" --status-cmd "$ORACLE_STUB_DIR/leak-oracle.sh"

assert "C1: exit 0" test "$RC" -eq 0
assert "C2: _lane-done backing-task-status=terminal" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-done .*status=terminal"' _ "$OUT"
assert "C3: _lane-done classification RECLAIMABLE" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-done .*classification=RECLAIMABLE"' _ "$OUT"
assert "C4: HEADROOM line shows reclaimable=1" \
    bash -c 'printf "%s\n" "$1" | grep "^HEADROOM" | grep -q "reclaimable=1"' _ "$OUT"
assert "C5: _lane-pending backing-task-status=non-terminal" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-pending .*status=non-terminal"' _ "$OUT"
assert "C6: _lane-pending classification PRESERVED-OK (not reclaimable-via-terminal)" \
    bash -c 'printf "%s\n" "$1" | grep -q "lane=_lane-pending .*classification=PRESERVED-OK"' _ "$OUT"

test_summary
