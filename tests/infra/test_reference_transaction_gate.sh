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
TWO="2222222222222222222222222222222222222222"
# A GENUINE advance: existing main (ONE) -> a DIFFERENT existing commit (TWO).
# Both oids nonzero and distinct, so it passes the genuine-advance filter and IS
# gated. (Housekeeping shapes — no-op / create / delete — are exercised separately
# in scenarios g/h/i below and must NEVER be gated.)
MAIN_LINE="$ONE $TWO refs/heads/main"
BRANCH_LINE="$ONE $TWO refs/heads/task/foo"

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

# ===========================================================================
# Genuine-advance filter (the advance-filter change): only old!=new AND both
# nonzero is a landing. Housekeeping transactions on refs/heads/main — no-op
# (X->X), creation (0000->X), deletion (X->0000) — must ALWAYS be allowed and
# never logged, even under ENFORCE, so that `git gc` / `git pack-refs` /
# `git fetch` are never aborted. These are the cases the 4-day warn-only window
# showed (~48 events) that would otherwise wedge the repo when enforcing.
# ===========================================================================

# -- (g) no-op self-move (old==new) -> allowed + silent, even under ENFORCE ----
echo ""
echo "--- (g) no-op main move (X->X) -> exit 0, NOT gated, even under ENFORCE ---"
rm -f "$SENTINEL" "$LOG"
drive prepared "$ONE $ONE refs/heads/main" REIFY_MAIN_GATE_ENFORCE=1
assert "(g) no-op move never aborts even under ENFORCE (exit 0)" test "$DRIVE_RC" -eq 0
assert "(g) no-op move leaves no log (housekeeping, not a landing)" bash -c "! test -f '$LOG'"

# -- (h) ref creation (old all-zero) -> allowed + silent, even under ENFORCE ---
echo ""
echo "--- (h) main ref creation (0000->X) -> exit 0, NOT gated, even under ENFORCE ---"
rm -f "$SENTINEL" "$LOG"
drive prepared "$ZERO $ONE refs/heads/main" REIFY_MAIN_GATE_ENFORCE=1
assert "(h) creation never aborts even under ENFORCE (exit 0)" test "$DRIVE_RC" -eq 0
assert "(h) creation leaves no log (pack-refs/gc churn, not a landing)" bash -c "! test -f '$LOG'"

# -- (i) ref deletion (new all-zero) -> allowed + silent, even under ENFORCE ---
echo ""
echo "--- (i) main ref deletion (X->0000) -> exit 0, NOT gated, even under ENFORCE ---"
rm -f "$SENTINEL" "$LOG"
drive prepared "$ONE $ZERO refs/heads/main" REIFY_MAIN_GATE_ENFORCE=1
assert "(i) deletion never aborts even under ENFORCE (exit 0)" test "$DRIVE_RC" -eq 0
assert "(i) deletion leaves no log (pack-refs/gc churn, not a landing)" bash -c "! test -f '$LOG'"

# -- (j) a housekeeping move does NOT consume a pending sentinel ---------------
# A real advance is gated and consumes the sentinel; a housekeeping move must
# leave the sentinel intact so the next GENUINE advance is still recognised as
# sanctioned (the filter short-circuits before the consume step).
echo ""
echo "--- (j) housekeeping move leaves a pending sentinel intact ---"
rm -f "$LOG"; : > "$SENTINEL"
drive prepared "$ONE $ONE refs/heads/main"
assert "(j) no-op move exits 0" test "$DRIVE_RC" -eq 0
assert "(j) no-op move does NOT consume the sentinel" bash -c "test -e '$SENTINEL'"
# ...and the subsequent genuine advance then consumes it (sanctioned).
drive prepared "$ONE $TWO refs/heads/main"
assert "(j) following genuine advance consumes the sentinel" bash -c "! test -e '$SENTINEL'"
assert "(j) following genuine advance logged sanctioned" bash -c "grep -q 'sanctioned main move' '$LOG'"

test_summary
