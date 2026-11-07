#!/usr/bin/env bash
# Infrastructure test for the refs/stash arm of hooks/reference-transaction —
# the fail-closed guard against `git stash` in a shared checkout (task 5981).
#
# WHY THE GUARD EXISTS: refs/stash is ONE ref in the shared .git. Git treats only
# HEAD, refs/worktree/*, refs/bisect/* and refs/rewritten/* as per-worktree, so
# every warm lane (a linked worktree of /home/leo/src/reify/.git) pushes onto ONE
# host-wide LIFO stack — esc-5785-6, where nine entries from different tasks
# accreted over ~1 month.
#
# WHAT IS ASSERTED, AND WHAT IS DELIBERATELY NOT. The guard covers the PUSH
# direction only. Measured (git 2.43.0, throwaway repos): `git stash pop` /
# `git stash drop` of a NON-LAST entry fire no reference-transaction hook at all
# (git rewrites the stash reflog rather than the ref), and a last-entry pop
# applies to the working tree BEFORE its ref deletion fires — so the injection
# direction is unreachable by any hook. Scenario (g) pins the deletion direction
# as ungated ON PURPOSE, and scenario (k) pins that the refusal message DISCLOSES
# that gap rather than claiming push/pop/drop/clear coverage.
#
# HERMETICITY IS NON-NEGOTIABLE: every `git stash` below runs inside an
# `mktemp -d` repo with its own `git init`. The real repository's shared stash
# stack is LIVE (measured: 1 entry during this task's planning) — no assertion
# here reads it and no command here mutates it.
#
# Two fixture flavours:
#   FIXTURE A — the hook driven DIRECTLY with fixture stdin lines (the
#               test_reference_transaction_gate.sh harness), for the enum arms:
#               deletion-ungated, non-'prepared'-never-aborts, durable switch
#               resolution, switch independence.
#   FIXTURE B — a real `git stash push` under `core.hooksPath=hooks`, for the
#               things the driven harness cannot express end-to-end: the
#               working-tree-preservation property, the merge-park regression
#               guard, and the refusal message an agent actually sees.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== reference-transaction refs/stash guard (task 5981) ==="

# install_hooks <dir> — copy the hook + lib into <dir>/hooks (the hook resolves
# the lib relative to its own location, so this works regardless of CWD).
install_hooks() {
    mkdir -p "$1/hooks"
    cp "$REPO_ROOT/hooks/reference-transaction" "$1/hooks/"
    cp "$REPO_ROOT/hooks/main-gate-lib.sh" "$1/hooks/"
    chmod +x "$1/hooks/reference-transaction"
}

# ═══════════════════════════════════════════════════════════════════════════
# FIXTURE A — driven hook, fixture stdin lines.
# ═══════════════════════════════════════════════════════════════════════════
FIX_A="$(mktemp -d)"; _TMPDIRS+=("$FIX_A")
install_hooks "$FIX_A"
git -C "$FIX_A" init -q
git -C "$FIX_A" config user.email test@test.com
git -C "$FIX_A" config user.name Test

HOOK="$FIX_A/hooks/reference-transaction"
LOG_A="$FIX_A/.git/reify-main-gate.log"
ZERO="0000000000000000000000000000000000000000"
ONE="1111111111111111111111111111111111111111"
TWO="2222222222222222222222222222222222222222"
# A stash PUSH as git actually presents it: old is ALL-ZERO even when the stack
# is already non-empty (measured — see scenario (f)), new is the stash commit.
STASH_PUSH_LINE="$ZERO $TWO refs/stash"
# A stash DELETION (`git stash clear`, a last-entry drop): old is the entry, new
# all-zero. Deliberately UNGATED — scenario (g).
STASH_DELETE_LINE="$ONE $ZERO refs/stash"
# A genuine main advance, used for the switch-independence scenarios.
MAIN_LINE="$ONE $TWO refs/heads/main"

# drive <state> <stdin-line> [ENV=VAL ...] — run the hook in FIXTURE A; sets
# DRIVE_RC and DRIVE_OUT (stdout+stderr). Ambient REIFY_* switches are stripped
# so a caller's environment can never arm or disarm a scenario.
drive() {
    local state="$1" line="$2"; shift 2
    local rc=0
    DRIVE_OUT="$( cd "$FIX_A" && printf '%s\n' "$line" \
        | env -u REIFY_STASH_GUARD_ENFORCE -u REIFY_STASH_GUARD_BYPASS \
              -u REIFY_MAIN_GATE_ENFORCE -u REIFY_MAIN_GATE_BYPASS \
              "$@" bash "$HOOK" "$state" 2>&1 )" || rc=$?
    DRIVE_RC=$rc
}

STASH_ENFORCE_FLAG="$FIX_A/.git/reify-stash-guard-enforce"
STASH_BYPASS_FLAG="$FIX_A/.git/reify-stash-guard-bypass"
reset_durable() {
    git -C "$FIX_A" config --unset reify.stashGuard.enforce 2>/dev/null || true
    git -C "$FIX_A" config --unset reify.stashGuard.bypass  2>/dev/null || true
    git -C "$FIX_A" config --unset reify.mainGate.enforce   2>/dev/null || true
    git -C "$FIX_A" config --unset reify.mainGate.bypass    2>/dev/null || true
    rm -f "$STASH_ENFORCE_FLAG" "$STASH_BYPASS_FLAG"
    rm -f "$FIX_A/.git/reify-main-gate-enforce" "$FIX_A/.git/reify-main-gate-bypass"
}

# ═══════════════════════════════════════════════════════════════════════════
# FIXTURE B — a REAL `git stash push` with the hook wired via core.hooksPath.
# ═══════════════════════════════════════════════════════════════════════════
FIX_B="$(mktemp -d)"; _TMPDIRS+=("$FIX_B")
install_hooks "$FIX_B"
git -C "$FIX_B" init -q -b main
git -C "$FIX_B" config user.email test@test.com
git -C "$FIX_B" config user.name Test
git -C "$FIX_B" config core.hooksPath hooks
printf 'committed\n' > "$FIX_B/wip.txt"
git -C "$FIX_B" add wip.txt
git -C "$FIX_B" commit -q -m "base commit"
LOG_B="$FIX_B/.git/reify-main-gate.log"

# git_b [ENV=VAL ...] -- <git args ...> — run git inside FIXTURE B; sets B_RC and
# B_OUT (stdout+stderr merged, which is what an agent sees at its terminal).
git_b() {
    local envs=() rc=0
    while [ "$#" -gt 0 ] && [ "$1" != "--" ]; do envs+=("$1"); shift; done
    [ "${1:-}" = "--" ] && shift
    B_OUT="$( cd "$FIX_B" && env -u REIFY_STASH_GUARD_ENFORCE -u REIFY_STASH_GUARD_BYPASS \
              -u REIFY_MAIN_GATE_ENFORCE -u REIFY_MAIN_GATE_BYPASS \
              "${envs[@]+${envs[@]}}" git "$@" 2>&1 )" || rc=$?
    B_RC=$rc
}
dirty_b() { printf '%s\n' "$1" > "$FIX_B/wip.txt"; }
stash_count_b() { git -C "$FIX_B" stash list | wc -l | tr -d ' '; }

# -- (a) ENFORCE on: a real `git stash push` is REFUSED --------------------------
echo ""
echo "--- (a) real 'git stash push' under ENFORCE -> non-zero, transaction aborted ---"
rm -f "$LOG_B"
dirty_b "modified-a"
PRE_LIST="$(git -C "$FIX_B" stash list)"
PRE_CONTENT="$(cat "$FIX_B/wip.txt")"
git_b REIFY_STASH_GUARD_ENFORCE=1 -- stash push -m "guarded push"
REFUSAL_OUT="$B_OUT"
assert "(a) enforced push exits non-zero (measured rc=128)" test "$B_RC" -ne 0
assert "(a) git reports the aborted ref transaction" \
    bash -c "printf '%s' \"\$1\" | grep -q 'ref updates aborted by hook'" _ "$REFUSAL_OUT"

# -- (b) LOAD-BEARING: the refusal preserves the working tree AND the stack -----
# This is the property that makes a fail-closed guard safe to arm: refusing the
# ref update leaves the WIP exactly where it was, so an agent loses nothing.
echo ""
echo "--- (b) refusal preserves the working tree and leaves the stack unchanged ---"
assert "(b) working tree still holds the modified content" \
    bash -c "test \"\$(cat '$FIX_B/wip.txt')\" = '$PRE_CONTENT'"
assert "(b) 'git stash list' byte-identical to the pre-refusal snapshot" \
    bash -c "test \"\$(git -C '$FIX_B' stash list)\" = \"\$1\"" _ "$PRE_LIST"

# -- (k) REFUSAL-MESSAGE HONESTY (behavioural: the hook's real stderr) ----------
# Short substring probes, not a prose pin. The task description directs a message
# claiming push/pop/drop/clear coverage, which is measurably FALSE; probe 4 is
# what stops that false coverage claim shipping in the guard's own stderr.
echo ""
echo "--- (k) refusal message is actionable and HONEST about its reach ---"
msg_has() { printf '%s' "$REFUSAL_OUT" | grep -qi -- "$1"; }
assert "(k) names refs/stash as shared, NOT per-worktree" msg_has 'not per-worktree'
assert "(k) tells the agent to commit instead" msg_has 'commit'
assert "(k) states the working tree was not touched" msg_has 'working tree'
assert "(k) discloses the pop/drop coverage gap" msg_has 'pop'
assert "(k) names the break-glass switch" msg_has 'REIFY_STASH_GUARD_BYPASS'

# -- (c) MERGE-PARK REGRESSION GUARD (dark-factory git_ops.py:11482/11583) ------
# `git stash create` + `git stash apply <ref>` never write refs/stash (measured:
# neither fires a refs/stash transaction), so dark-factory's MERGE_PARK_REF path
# must keep working with the guard ARMED. If a future edit widened the arm to any
# stash-ish ref, this is what would catch it.
echo ""
echo "--- (c) 'git stash create' / 'apply <ref>' still work with ENFORCE on ---"
# Self-sufficient preconditions (a dirty tree, and its own stack snapshot) so
# this block asserts the merge-park path on its own terms rather than inheriting
# whatever (a) left behind.
dirty_b "$PRE_CONTENT"
C_PRE_LIST="$(git -C "$FIX_B" stash list)"
git_b REIFY_STASH_GUARD_ENFORCE=1 -- stash create "merge-park probe"
CREATE_RC="$B_RC"; CREATE_OID="$B_OUT"
assert "(c) 'stash create' exits 0 under ENFORCE" test "$CREATE_RC" -eq 0
assert "(c) 'stash create' printed a 40-hex oid" \
    bash -c "printf '%s' \"\$1\" | grep -Eq '^[0-9a-f]{40}\$'" _ "$CREATE_OID"
assert "(c) 'stash create' left the stack unchanged" \
    bash -c "test \"\$(git -C '$FIX_B' stash list)\" = \"\$1\"" _ "$C_PRE_LIST"
git_b REIFY_STASH_GUARD_ENFORCE=1 -- update-ref refs/dark-factory/merge-park "$CREATE_OID"
assert "(c) parking the oid at refs/dark-factory/merge-park exits 0" test "$B_RC" -eq 0
git -C "$FIX_B" checkout -q -- wip.txt   # clean tree so the apply can land
git_b REIFY_STASH_GUARD_ENFORCE=1 -- stash apply refs/dark-factory/merge-park
assert "(c) 'stash apply <ref>' exits 0 under ENFORCE" test "$B_RC" -eq 0
assert "(c) 'stash apply <ref>' left the stack unchanged" \
    bash -c "test \"\$(git -C '$FIX_B' stash list)\" = \"\$1\"" _ "$C_PRE_LIST"
assert "(c) the parked WIP was restored to the working tree" \
    bash -c "test \"\$(cat '$FIX_B/wip.txt')\" = '$PRE_CONTENT'"

# -- (d) BYPASS is the break-glass: the push lands ------------------------------
echo ""
echo "--- (d) BYPASS permits the push even with ENFORCE on ---"
D_PRE_COUNT="$(stash_count_b)"
git_b REIFY_STASH_GUARD_BYPASS=1 REIFY_STASH_GUARD_ENFORCE=1 -- stash push -m "bypassed push"
assert "(d) push under BYPASS exits 0" test "$B_RC" -eq 0
assert "(d) the entry landed on the stack" test "$(stash_count_b)" -eq "$((D_PRE_COUNT + 1))"
git_b REIFY_STASH_GUARD_BYPASS=1 REIFY_STASH_GUARD_ENFORCE=1 -- stash clear
assert "(d) 'stash clear' under BYPASS exits 0" test "$B_RC" -eq 0
assert "(d) the stack is empty again" test "$(stash_count_b)" -eq 0

# -- (e) WARN-ONLY DEFAULT: observe before enforcing ----------------------------
echo ""
echo "--- (e) default (neither switch set) -> push allowed but LOGGED ---"
rm -f "$LOG_B"
dirty_b "modified-e"
git_b -- stash push -m "warn-only push"
assert "(e) default posture allows the push (exit 0)" test "$B_RC" -eq 0
assert "(e) the entry landed" test "$(stash_count_b)" -eq 1
assert "(e) the event was logged as a stash-guard event" \
    bash -c "grep -q 'stash-guard' '$LOG_B'"

# -- (f) ZERO-OLD TRAP: a push onto a NON-EMPTY stack is still refused ----------
# Measured twice: git presents old=0000000000... on refs/stash even when the
# stack already holds entries. Copying the main arm's `[ -z "${_old//0/}" ]
# && continue` housekeeping filter into the stash arm would make this guard
# SILENTLY NEVER FIRE while still logging as installed — a dead instrument that
# reads as protection. This scenario is the anti-regression net for that.
echo ""
echo "--- (f) push onto a NON-EMPTY stack is still refused (zero-old trap) ---"
assert "(f) precondition: the stack is non-empty (not a vacuous pass)" test "$(stash_count_b)" -eq 1
dirty_b "modified-f"
git_b REIFY_STASH_GUARD_ENFORCE=1 -- stash push -m "second push"
assert "(f) the SECOND push is refused too (exit non-zero)" test "$B_RC" -ne 0
assert "(f) the stack did not grow" test "$(stash_count_b)" -eq 1
assert "(f) the working tree still holds the modified content" \
    bash -c "test \"\$(cat '$FIX_B/wip.txt')\" = 'modified-f'"
# ... and the same shape driven directly, so the enum arm is pinned too.
reset_durable; rm -f "$LOG_A"
drive prepared "$STASH_PUSH_LINE" REIFY_STASH_GUARD_ENFORCE=1
assert "(f) driven: all-zero-old stash push line refused under ENFORCE (exit 1)" test "$DRIVE_RC" -eq 1

# -- (g) DELETION DIRECTION DELIBERATELY UNGATED --------------------------------
# `git stash clear` and a last-entry drop reach a ref DELETION (old=<oid>,
# new=0000...). Measured reasons this stays ungated: (1) refusing a deletion
# cannot prevent a pop's injection, because the working-tree APPLY precedes the
# ref deletion (and a non-last pop fires no hook at all); (2) an aborted
# last-entry drop leaves a partial state — git had already emptied the stash
# reflog while refs/stash still pointed at the entry; (3) it would block exactly
# the operator cleanup that drained the nine accreted entries on 2026-08-02/03.
echo ""
echo "--- (g) stash ref DELETION is never gated (operator cleanup must work) ---"
reset_durable; rm -f "$LOG_A"
drive prepared "$STASH_DELETE_LINE" REIFY_STASH_GUARD_ENFORCE=1
assert "(g) deletion allowed even under ENFORCE (exit 0)" test "$DRIVE_RC" -eq 0
assert "(g) deletion leaves no log (not a push, nothing to warn about)" \
    bash -c "! test -f '$LOG_A'"

# -- (h) NON-'prepared' STATES NEVER ABORT --------------------------------------
echo ""
echo "--- (h) 'committed' state never aborts (git ignores the code there) ---"
reset_durable; rm -f "$LOG_A"
drive committed "$STASH_PUSH_LINE" REIFY_STASH_GUARD_ENFORCE=1
assert "(h) committed state exits 0 under ENFORCE" test "$DRIVE_RC" -eq 0
assert "(h) committed state leaves no log" bash -c "! test -f '$LOG_A'"

# -- (i) SWITCH INDEPENDENCE (both directions) ----------------------------------
# The two staged rollouts must never be coupled: arming the stash guard must not
# arm the landing gate, and arming the landing gate must not arm the stash guard.
echo ""
echo "--- (i) the stash switch and the main-gate switch are independent ---"
reset_durable; rm -f "$LOG_A" "$FIX_A/.git/reify-main-gate-ok"
drive prepared "$MAIN_LINE" REIFY_STASH_GUARD_ENFORCE=1
assert "(i) stash ENFORCE does not arm the main gate (exit 0)" test "$DRIVE_RC" -eq 0
reset_durable; rm -f "$LOG_A"
drive prepared "$STASH_PUSH_LINE" REIFY_MAIN_GATE_ENFORCE=1
assert "(i) main-gate ENFORCE does not arm the stash guard (exit 0)" test "$DRIVE_RC" -eq 0

# -- (j) DURABLE SWITCH RESOLUTION (the reason the indirection exists) ----------
# A long-lived orchestrator worker cannot pass an env var down to the git
# subprocess that fires this hook, so the policy must survive WITHOUT one.
# Precedence: env (override) -> git config reify.stashGuard.* -> flag file.
echo ""
echo "--- (j) durable enforce/bypass resolution with NO env var ---"
reset_durable; rm -f "$LOG_A"
git -C "$FIX_A" config reify.stashGuard.enforce true
drive prepared "$STASH_PUSH_LINE"
assert "(j) git-config enforce refuses with no env var (exit 1)" test "$DRIVE_RC" -eq 1
reset_durable

rm -f "$LOG_A"
: > "$STASH_ENFORCE_FLAG"
drive prepared "$STASH_PUSH_LINE"
assert "(j) flag-file enforce refuses with no env var (exit 1)" test "$DRIVE_RC" -eq 1
reset_durable

rm -f "$LOG_A"
git -C "$FIX_A" config reify.stashGuard.enforce true
drive prepared "$STASH_PUSH_LINE" REIFY_STASH_GUARD_ENFORCE=0
assert "(j) env=0 overrides a durable enforce-on (exit 0)" test "$DRIVE_RC" -eq 0
assert "(j) still logged in the warn-only posture" bash -c "grep -q 'stash-guard' '$LOG_A'"
reset_durable

rm -f "$LOG_A"
git -C "$FIX_A" config reify.stashGuard.enforce true
git -C "$FIX_A" config reify.stashGuard.bypass true
drive prepared "$STASH_PUSH_LINE"
assert "(j) durable bypass wins over durable enforce (exit 0)" test "$DRIVE_RC" -eq 0
assert "(j) bypass logged" bash -c "grep -q 'stash-guard' '$LOG_A'"
reset_durable

test_summary
