#!/usr/bin/env bash
# Infrastructure test for Fix 2 (main-gate-hardening): hooks/reference-transaction
# is the tripwire that detects moves of refs/heads/main made by
# reset / update-ref / fast-forward — the gap that pre-commit and
# pre-merge-commit miss (they fire only when a commit / merge commit is created).
#
# The hook is driven DIRECTLY with fixture stdin inside a throwaway temp git repo,
# so the real repository's main ref is never touched. The hook + lib are copied
# into the fixture's hooks/ dir; the hook resolves the lib relative to its own
# location, so this works regardless of CWD or core.hooksPath.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== reference-transaction main-gate tripwire (Fix 2) ==="

# Throwaway git repo containing just the hook + lib.
FIX="$(mktemp -d)"; _TMPDIRS+=("$FIX")
mkdir -p "$FIX/hooks"
cp "$REPO_ROOT/hooks/reference-transaction" "$FIX/hooks/"
cp "$REPO_ROOT/hooks/main-gate-lib.sh" "$FIX/hooks/"
chmod +x "$FIX/hooks/reference-transaction"
git -C "$FIX" init -q
git -C "$FIX" config user.email test@test.com
git -C "$FIX" config user.name Test

HOOK="$FIX/hooks/reference-transaction"
SENTINEL="$FIX/.git/reify-main-gate-ok"
LOG="$FIX/.git/reify-main-gate.log"
ZERO="0000000000000000000000000000000000000000"
ONE="1111111111111111111111111111111111111111"
MAIN_LINE="$ZERO $ONE refs/heads/main"
BRANCH_LINE="$ZERO $ONE refs/heads/task/foo"

# drive <state> <stdin-line> [ENV=VAL ...] — run the hook in the fixture; sets DRIVE_RC.
drive() {
    local state="$1" line="$2"; shift 2
    local rc=0
    ( cd "$FIX" && printf '%s\n' "$line" | env "$@" bash "$HOOK" "$state" ) || rc=$?
    DRIVE_RC=$rc
}

# -- (a) main move, no sentinel, default (warn-only) -> exit 0 + UNSANCTIONED ---
echo ""
echo "--- (a) unsanctioned main move, default warn-only -> exit 0, logged ---"
rm -f "$SENTINEL" "$LOG"
drive prepared "$MAIN_LINE"
assert "(a) warn-only allows the move (exit 0)" test "$DRIVE_RC" -eq 0
assert "(a) UNSANCTIONED logged" bash -c "grep -q 'UNSANCTIONED main move' '$LOG'"

# -- (b) main move, no sentinel, ENFORCE=1 -> exit 1 (aborts the transaction) --
echo ""
echo "--- (b) unsanctioned main move, ENFORCE=1 -> exit 1 (abort) ---"
rm -f "$SENTINEL" "$LOG"
drive prepared "$MAIN_LINE" REIFY_MAIN_GATE_ENFORCE=1
assert "(b) enforce aborts the transaction (exit 1)" test "$DRIVE_RC" -eq 1
assert "(b) UNSANCTIONED logged before abort" bash -c "grep -q 'UNSANCTIONED main move' '$LOG'"

# -- (c) sentinel present -> exit 0, consumed (one-shot), sanctioned logged ----
echo ""
echo "--- (c) sanctioned main move (sentinel present) -> exit 0, consumed ---"
rm -f "$LOG"; : > "$SENTINEL"
drive prepared "$MAIN_LINE"
assert "(c) sanctioned move allowed (exit 0)" test "$DRIVE_RC" -eq 0
assert "(c) sentinel consumed (one-shot)" bash -c "! test -e '$SENTINEL'"
assert "(c) sanctioned logged" bash -c "grep -q 'sanctioned main move' '$LOG'"
# A sanctioned move passes even under ENFORCE.
rm -f "$LOG"; : > "$SENTINEL"
drive prepared "$MAIN_LINE" REIFY_MAIN_GATE_ENFORCE=1
assert "(c2) sanctioned move passes even under ENFORCE (exit 0)" test "$DRIVE_RC" -eq 0
assert "(c2) sentinel consumed under ENFORCE" bash -c "! test -e '$SENTINEL'"

# -- (d) BYPASS=1 -> exit 0 even with no sentinel and ENFORCE on ---------------
echo ""
echo "--- (d) BYPASS=1 -> exit 0 (break-glass), logged ---"
rm -f "$SENTINEL" "$LOG"
drive prepared "$MAIN_LINE" REIFY_MAIN_GATE_BYPASS=1 REIFY_MAIN_GATE_ENFORCE=1
assert "(d) bypass allows even with ENFORCE on (exit 0)" test "$DRIVE_RC" -eq 0
assert "(d) bypass logged" bash -c "grep -q 'bypass' '$LOG'"

# -- (e) non-main ref -> exit 0, nothing logged (worktree branches untouched) --
echo ""
echo "--- (e) non-main ref -> exit 0, silent (no log) ---"
rm -f "$SENTINEL" "$LOG"
drive prepared "$BRANCH_LINE" REIFY_MAIN_GATE_ENFORCE=1
assert "(e) non-main ref allowed even under ENFORCE (exit 0)" test "$DRIVE_RC" -eq 0
assert "(e) non-main ref leaves no log" bash -c "! test -f '$LOG'"

# -- (f) non-'prepared' state is a post-facto notification: ignored, exit 0 ----
# 'committed'/'aborted' exit codes are ignored by git, so the hook must never
# abort on them — even an unsanctioned main move under ENFORCE must pass.
echo ""
echo "--- (f) non-'prepared' state -> exit 0, silent ---"
rm -f "$SENTINEL" "$LOG"
drive committed "$MAIN_LINE" REIFY_MAIN_GATE_ENFORCE=1
assert "(f) committed state never aborts (exit 0)" test "$DRIVE_RC" -eq 0
assert "(f) committed state leaves no log" bash -c "! test -f '$LOG'"

test_summary
