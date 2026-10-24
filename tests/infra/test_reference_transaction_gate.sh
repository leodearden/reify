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

# Durable ENFORCE/BYPASS sources (see the durable-switch block below).
ENFORCE_FLAG="$FIX/.git/reify-main-gate-enforce"
BYPASS_FLAG="$FIX/.git/reify-main-gate-bypass"
reset_durable() {
    git -C "$FIX" config --unset reify.mainGate.enforce 2>/dev/null || true
    git -C "$FIX" config --unset reify.mainGate.bypass  2>/dev/null || true
    rm -f "$ENFORCE_FLAG" "$BYPASS_FLAG"
}

# -- (a) main move, no sentinel, default (warn-only) -> exit 0 + UNSANCTIONED ---
echo ""
echo "--- (a) unsanctioned main move, default warn-only -> exit 0, logged ---"
rm -f "$SENTINEL" "$LOG"
drive prepared "$MAIN_LINE"
assert "(a) warn-only allows the move (exit 0)" test "$DRIVE_RC" -eq 0
assert "(a) UNSANCTIONED logged" bash -c "grep -q 'UNSANCTIONED main move' '$LOG'"
assert "(a) no sentinel, so no unbound-sentinel line" bash -c "! grep -qF 'unbound (empty) sentinel' '$LOG'"

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
rm -f "$LOG"; printf '%s\n' "$ONE" > "$SENTINEL"
drive prepared "$MAIN_LINE"
assert "(c) sanctioned move allowed (exit 0)" test "$DRIVE_RC" -eq 0
assert "(c) sentinel consumed (one-shot)" bash -c "! test -e '$SENTINEL'"
assert "(c) sanctioned logged" bash -c "grep -q 'sanctioned main move' '$LOG'"
# A sanctioned move passes even under ENFORCE.
rm -f "$LOG"; printf '%s\n' "$ONE" > "$SENTINEL"
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
rm -f "$LOG"; printf '%s\n' "$ONE" > "$SENTINEL"
drive prepared "$ONE $ONE refs/heads/main"
assert "(j) no-op move exits 0" test "$DRIVE_RC" -eq 0
assert "(j) no-op move does NOT consume the sentinel" bash -c "test -e '$SENTINEL'"
# ...and the subsequent genuine advance then consumes it (sanctioned).
drive prepared "$ONE $TWO refs/heads/main"
assert "(j) following genuine advance consumes the sentinel" bash -c "! test -e '$SENTINEL'"
assert "(j) following genuine advance logged sanctioned" bash -c "grep -q 'sanctioned main move' '$LOG'"

# ===========================================================================
# Token table (D9): the sentinel carries the main tip it sanctions a move FROM.
#   matched token (== the move's old oid)  -> consumed, sanctioned       (c, j)
#   empty (unbound) token                  -> discarded, UNSANCTIONED     (s)
#   stale token (any other oid)            -> discarded, UNSANCTIONED     (t, t2)
#   bypass                                 -> sentinel untouched          (u)
# ===========================================================================

# -- (s) empty sentinel sanctions nothing -> UNSANCTIONED, logged unbound ------
echo ""
echo "--- (s) empty sentinel, ENFORCE=1 -> exit 1, consumed, UNSANCTIONED ---"
reset_durable; rm -f "$SENTINEL" "$LOG"; : > "$SENTINEL"
drive prepared "$MAIN_LINE" REIFY_MAIN_GATE_ENFORCE=1
assert "(s) empty sentinel does not sanction the move (exit 1)" test "$DRIVE_RC" -eq 1
assert "(s) empty sentinel consumed" bash -c "! test -e '$SENTINEL'"
assert "(s) move logged UNSANCTIONED" grep -qF "UNSANCTIONED main move $ONE -> $TWO" "$LOG"
assert "(s) no sanction line logged" bash -c "! grep -q 'sanctioned' '$LOG'"
assert "(s) empty sentinel logged as unbound, not as no sentinel" grep -qF "unbound (empty) sentinel discarded" "$LOG"

# -- (t) stale token under ENFORCE -> discarded, UNSANCTIONED, abort -----------
echo ""
echo "--- (t) stale token (bound to another tip), ENFORCE=1 -> exit 1, discarded ---"
reset_durable; rm -f "$SENTINEL" "$LOG"; printf '%s\n' "$TWO" > "$SENTINEL"
drive prepared "$MAIN_LINE" REIFY_MAIN_GATE_ENFORCE=1
assert "(t) stale token does not sanction the move (exit 1)" test "$DRIVE_RC" -eq 1
assert "(t) stale token removed" bash -c "! test -e '$SENTINEL'"
assert "(t) stale token logged as discarded" grep -qF "stale sentinel $TWO discarded" "$LOG"
assert "(t) move logged UNSANCTIONED" grep -qF "UNSANCTIONED main move $ONE -> $TWO" "$LOG"

# -- (t2) stale token warn-only -> discarded, UNSANCTIONED, allowed ------------
echo ""
echo "--- (t2) stale token, default warn-only -> exit 0, discarded ---"
reset_durable; rm -f "$SENTINEL" "$LOG"; printf '%s\n' "$TWO" > "$SENTINEL"
drive prepared "$MAIN_LINE"
assert "(t2) warn-only allows the move (exit 0)" test "$DRIVE_RC" -eq 0
assert "(t2) stale token removed" bash -c "! test -e '$SENTINEL'"
assert "(t2) stale token logged as discarded" grep -qF "stale sentinel $TWO discarded" "$LOG"
assert "(t2) move logged UNSANCTIONED" grep -qF "UNSANCTIONED main move $ONE -> $TWO" "$LOG"

# -- (u) bypass returns before the sentinel is read: it stays untouched --------
echo ""
echo "--- (u) stale token + BYPASS=1 + ENFORCE=1 -> exit 0, sentinel untouched ---"
reset_durable; rm -f "$SENTINEL" "$LOG"; printf '%s\n' "$TWO" > "$SENTINEL"
drive prepared "$MAIN_LINE" REIFY_MAIN_GATE_BYPASS=1 REIFY_MAIN_GATE_ENFORCE=1
assert "(u) bypass allows the move (exit 0)" test "$DRIVE_RC" -eq 0
assert "(u) bypass logged" grep -qF "bypass main move $ONE -> $TWO" "$LOG"
assert "(u) bypass leaves the sentinel present" test -e "$SENTINEL"
assert "(u) bypass leaves the sentinel's token unchanged" test "$(cat "$SENTINEL" 2>/dev/null)" = "$TWO"
reset_durable; rm -f "$SENTINEL" "$LOG"

# ===========================================================================
# Durable ENFORCE/BYPASS switch (task 4367 step 2): the policy must survive
# WITHOUT an env var — the long-lived orchestrator merge worker cannot pass one
# to the update-ref subprocess that fires this hook. main_gate_{enforce,bypass}_on
# resolve, in precedence order: env var (override) -> git config
# reify.mainGate.{enforce,bypass} -> flag file <git-common-dir>/reify-main-gate-{enforce,bypass}.
# (Common git dir of the fixture is $FIX/.git, same place SENTINEL lives.)
# ===========================================================================

# -- (k) durable enforce via git config, NO env var -> exit 1 (abort) ----------
echo ""
echo "--- (k) durable enforce (git config reify.mainGate.enforce), no env -> exit 1 ---"
reset_durable; rm -f "$SENTINEL" "$LOG"
git -C "$FIX" config reify.mainGate.enforce true
drive prepared "$MAIN_LINE"
assert "(k) git-config enforce aborts with no env var (exit 1)" test "$DRIVE_RC" -eq 1
assert "(k) UNSANCTIONED logged before abort" bash -c "grep -q 'UNSANCTIONED main move' '$LOG'"
reset_durable

# -- (l) durable enforce via flag file, NO env var -> exit 1 (abort) -----------
echo ""
echo "--- (l) durable enforce (flag file), no env -> exit 1 ---"
reset_durable; rm -f "$SENTINEL" "$LOG"
: > "$ENFORCE_FLAG"
drive prepared "$MAIN_LINE"
assert "(l) flag-file enforce aborts with no env var (exit 1)" test "$DRIVE_RC" -eq 1
reset_durable

# -- (m) env var OVERRIDES a durable enforce-on -> exit 0 (warn-only) ----------
# REIFY_MAIN_GATE_ENFORCE set to a non-1 value forces enforcement OFF even when a
# durable source says on — the env var is the authoritative override.
echo ""
echo "--- (m) env REIFY_MAIN_GATE_ENFORCE=0 overrides durable enforce-on -> exit 0 ---"
reset_durable; rm -f "$SENTINEL" "$LOG"
git -C "$FIX" config reify.mainGate.enforce true
drive prepared "$MAIN_LINE" REIFY_MAIN_GATE_ENFORCE=0
assert "(m) env=0 overrides durable enforce-on (exit 0)" test "$DRIVE_RC" -eq 0
assert "(m) still logged UNSANCTIONED (warn-only)" bash -c "grep -q 'UNSANCTIONED main move' '$LOG'"
reset_durable

# -- (n) durable bypass (git config) wins even under durable enforce -> exit 0 -
echo ""
echo "--- (n) durable bypass (git config) + durable enforce -> exit 0 (allowed) ---"
reset_durable; rm -f "$SENTINEL" "$LOG"
git -C "$FIX" config reify.mainGate.enforce true
git -C "$FIX" config reify.mainGate.bypass true
drive prepared "$MAIN_LINE"
assert "(n) durable bypass allows even with durable enforce (exit 0)" test "$DRIVE_RC" -eq 0
assert "(n) bypass logged" bash -c "grep -q 'bypass' '$LOG'"
reset_durable

# ===========================================================================
# Non-fast-forward (history-rewrite) guard (esc-5279-3): main may only ever
# fast-forward. A `git commit --amend` / rebase / backward reset produces a
# NON-ff move that rewrites a landed commit which concurrent task branches (all
# share this one refs/heads/main) may already have rebased onto — stranding
# them on an orphaned ex-main commit. Rejected regardless of the verify sentinel
# (a verify-passing amend is still "sanctioned" by the sentinel check — this
# guard is exactly why that no longer suffices) and regardless of the ENFORCE
# staging; a genuine break-glass rollback still wins via BYPASS.
#
# These scenarios need REAL commits (the fake oids above make merge-base error,
# which fails OPEN by design and is covered by (a)-(n) staying green), so build
# a tiny history in the fixture:
#   C0 (base) -> C1 (child; a fast-forward target)
#   C0 (base) -> C2 (sibling of C1: an amend/rewrite target — C1 is NOT an
#                    ancestor of C2)
# ===========================================================================
echo ""
echo "--- non-fast-forward (history-rewrite) guard (esc-5279-3) ---"
reset_durable; rm -f "$SENTINEL" "$LOG"
git -C "$FIX" commit -q --allow-empty -m "C0 base"
C0="$(git -C "$FIX" rev-parse HEAD)"
git -C "$FIX" commit -q --allow-empty -m "C1 child"
C1="$(git -C "$FIX" rev-parse HEAD)"
git -C "$FIX" checkout -q -b _nff_sib "$C0"
git -C "$FIX" commit -q --allow-empty -m "C2 sibling (rewrite target)"
C2="$(git -C "$FIX" rev-parse HEAD)"
git -C "$FIX" checkout -q -

# -- (o) fast-forward main move (C0->C1) is NOT rejected by the guard ----------
# Sentinel present so it is sanctioned and exits 0 — proves a genuine ff landing
# still passes (the guard falls through to the existing sanction logic on rc 0).
rm -f "$LOG"; printf '%s\n' "$C0" > "$SENTINEL"
drive prepared "$C0 $C1 refs/heads/main"
assert "(o) fast-forward main move allowed (exit 0)" test "$DRIVE_RC" -eq 0
assert "(o) fast-forward move sanctioned (not flagged as rewrite)" bash -c "grep -q 'sanctioned main move' '$LOG'"
assert "(o) fast-forward move not logged as HISTORY-REWRITE" bash -c "! grep -q 'HISTORY-REWRITE' '$LOG'"

# -- (p) non-ff move (C1->C2) REJECTED even WITH a matched token (C1) present -
# The core case: a verify-passing `git commit --amend` sets the sentinel
# (pre-commit -> project-checks -> main_gate_mark), which today makes it
# "sanctioned". The non-ff guard rejects it anyway, and BEFORE the sentinel is
# consumed (it runs ahead of the sanction block), so the sentinel is left intact.
rm -f "$LOG"; printf '%s\n' "$C1" > "$SENTINEL"
drive prepared "$C1 $C2 refs/heads/main"
assert "(p) non-ff move rejected even with sentinel present (exit 1)" test "$DRIVE_RC" -eq 1
assert "(p) non-ff move logged HISTORY-REWRITE" bash -c "grep -q 'HISTORY-REWRITE (non-fast-forward)' '$LOG'"
assert "(p) non-ff rejection did NOT consume the sentinel" bash -c "test -e '$SENTINEL'"

# -- (q) non-ff move (C1->C2) with BYPASS -> allowed (break-glass rollback) ----
rm -f "$SENTINEL" "$LOG"
drive prepared "$C1 $C2 refs/heads/main" REIFY_MAIN_GATE_BYPASS=1
assert "(q) non-ff move allowed under BYPASS (exit 0)" test "$DRIVE_RC" -eq 0
assert "(q) non-ff BYPASS logged as bypass, not rewrite" bash -c "grep -q 'bypass' '$LOG' && ! grep -q 'HISTORY-REWRITE' '$LOG'"

# -- (r) non-ff move (C1->C2) rejected with NO sentinel and ENFORCE off --------
# Proves the guard is always-on (independent of the ENFORCE staging): a rewrite
# aborts even in the default warn-only posture, unlike the sanction dimension.
reset_durable; rm -f "$SENTINEL" "$LOG"
drive prepared "$C1 $C2 refs/heads/main"
assert "(r) non-ff rejected in default warn-only posture (exit 1)" test "$DRIVE_RC" -eq 1
assert "(r) non-ff logged HISTORY-REWRITE (always-on)" bash -c "grep -q 'HISTORY-REWRITE (non-fast-forward)' '$LOG'"

# ===========================================================================
# Real-git boundary (D9 B5/B6): the hooks fired BY GIT through core.hooksPath,
# not by drive() — git's own stdin shape, env reaching the hook, and the real
# pre-commit mark site. One fresh fixture per scenario; its base commit is made
# before core.hooksPath is set, so no scenario inherits a sentinel or a log.
# ===========================================================================

# make_fix2 — fresh repo on main (one base commit) with the real hooks wired;
# sets FIX2, F2_SENTINEL, F2_LOG.
make_fix2() {
    FIX2="$(mktemp -d)"; _TMPDIRS+=("$FIX2")
    git -C "$FIX2" init -q -b main
    git -C "$FIX2" config user.email test@test.com
    git -C "$FIX2" config user.name Test
    git -C "$FIX2" commit -q --allow-empty -m base
    mkdir -p "$FIX2/hooks"
    cp "$REPO_ROOT/hooks/reference-transaction" "$REPO_ROOT/hooks/main-gate-lib.sh" \
       "$REPO_ROOT/hooks/pre-commit" "$FIX2/hooks/"
    chmod +x "$FIX2/hooks/reference-transaction" "$FIX2/hooks/pre-commit"
    # Plain cp, NO chmod: the committed exec bit is part of the contract, since
    # dark-factory execs this path from `sh -c`. A missing script is left for the
    # B8 assertions to report instead of aborting this file under set -e.
    mkdir -p "$FIX2/scripts"
    if [ -e "$REPO_ROOT/scripts/main-gate-mark.sh" ]; then
        cp "$REPO_ROOT/scripts/main-gate-mark.sh" "$FIX2/scripts/"
    fi
    git -C "$FIX2" config core.hooksPath "$FIX2/hooks"
    F2_SENTINEL="$FIX2/.git/reify-main-gate-ok"
    F2_LOG="$FIX2/.git/reify-main-gate.log"
}

# fix2_child <parent> — a loose commit object on top of <parent>; moves no ref.
fix2_child() {
    git -C "$FIX2" commit-tree -p "$1" -m "child of $1" "$(git -C "$FIX2" rev-parse "$1^{tree}")"
}

fix2_main() { git -C "$FIX2" rev-parse refs/heads/main; }
fix2_token() { cat "$F2_SENTINEL" 2>/dev/null; }

# fix2_mark — the in-process mark: the same public lib function pre-commit calls.
fix2_mark() { ( cd "$FIX2" && . hooks/main-gate-lib.sh && main_gate_mark ); }

# in_fix2 [ENV=VAL ...] <cmd...> — run from the fixture root; sets FIX2_RC.
in_fix2() {
    local rc=0
    ( cd "$FIX2" && env "$@" ) > "$FIX2/.git/in_fix2.out" 2>&1 || rc=$?
    FIX2_RC=$rc
}

# -- (B5) the mark binds the tip; a CAS move from that tip is sanctioned --------
echo ""
echo "--- (B5) bound mark + real update-ref from that tip, ENFORCE=1 -> sanctioned ---"
make_fix2
X="$(fix2_main)"; Y="$(fix2_child "$X")"
fix2_mark
assert "(B5) main_gate_mark writes the main tip as the token" test "$(fix2_token)" = "$X"
in_fix2 REIFY_MAIN_GATE_ENFORCE=1 git update-ref refs/heads/main "$Y" "$X"
assert "(B5) the move succeeds under ENFORCE (rc 0)" test "$FIX2_RC" -eq 0
assert "(B5) main advanced to Y" test "$(fix2_main)" = "$Y"
assert "(B5) sentinel consumed" bash -c "! test -e '$F2_SENTINEL'"
assert "(B5) log records the bound sanction" grep -qF "sanctioned main move $X -> $Y" "$F2_LOG"

# -- (B5') a docs commit on main through the real pre-commit is sanctioned ------
echo ""
echo "--- (B5') real pre-commit mark + commit on main, ENFORCE=1 -> sanctioned ---"
make_fix2
before="$(fix2_main)"
in_fix2 REIFY_MAIN_GATE_ENFORCE=1 git commit -q --allow-empty -m docs
after="$(fix2_main)"
assert "(B5') the commit succeeds under ENFORCE (rc 0)" test "$FIX2_RC" -eq 0
assert "(B5') main advanced" test "$before" != "$after"
assert "(B5') sentinel consumed" bash -c "! test -e '$F2_SENTINEL'"
assert "(B5') log records the bound sanction" grep -qF "sanctioned main move $before -> $after" "$F2_LOG"

# -- (B6) gate passed, commit aborted, main moved on: the leftover token is stale
# A mark at T0 that nothing consumed (bypass never touches the sentinel) must
# not sanction a later raw move from T1.
echo ""
echo "--- (B6) leftover token from an older tip + raw move, ENFORCE=1 -> refused ---"
make_fix2
T0="$(fix2_main)"; T1="$(fix2_child "$T0")"; T2="$(fix2_child "$T1")"
fix2_mark
in_fix2 REIFY_MAIN_GATE_BYPASS=1 git update-ref refs/heads/main "$T1" "$T0"
assert "(B6) setup: the bypassed move reached T1" test "$(fix2_main)" = "$T1"
assert "(B6) setup: the bypassed move left the token bound to T0" test "$(fix2_token)" = "$T0"
in_fix2 REIFY_MAIN_GATE_ENFORCE=1 git update-ref refs/heads/main "$T2" "$T1"
assert "(B6) the raw move is refused (rc != 0)" test "$FIX2_RC" -ne 0
assert "(B6) main stays at T1" test "$(fix2_main)" = "$T1"
assert "(B6) the stale token was discarded" bash -c "! test -e '$F2_SENTINEL'"
assert "(B6) log records the discard" grep -qF "stale sentinel $T0 discarded" "$F2_LOG"
assert "(B6) log records the move as UNSANCTIONED" grep -qF "UNSANCTIONED main move $T1 -> $T2" "$F2_LOG"

# ===========================================================================
# B8 — the dark-factory face. advance_main / recover_red_main run
# git.main_gate_mark_command before their CAS `git update-ref` and
# git.main_gate_unmark_command only after a failed one, each as
# `sh -c <cmd>` with cwd = project_root.
# ===========================================================================

# b8_seam <label> <mark-cmd> <unmark-cmd> — that call pattern in a fresh fixture:
# mark + CAS; mark + failed CAS + unmark; foreign-tip token + unmark.
b8_seam() {
    local label="$1" mark_cmd="$2" unmark_cmd="$3" t0 t1 t2
    make_fix2
    t0="$(fix2_main)"; t1="$(fix2_child "$t0")"; t2="$(fix2_child "$t1")"

    in_fix2 sh -c "$mark_cmd"
    assert "($label) mark exits 0" test "$FIX2_RC" -eq 0
    in_fix2 REIFY_MAIN_GATE_ENFORCE=1 git update-ref refs/heads/main "$t1" "$t0"
    assert "($label) the CAS move after a mark succeeds under ENFORCE" test "$FIX2_RC" -eq 0
    assert "($label) the move logs the bound sanction" grep -qF "sanctioned main move $t0 -> $t1" "$F2_LOG"
    assert "($label) the token was consumed" bash -c "! test -e '$F2_SENTINEL'"

    in_fix2 sh -c "$mark_cmd"
    in_fix2 REIFY_MAIN_GATE_ENFORCE=1 git update-ref refs/heads/main "$t2" "$t0"
    assert "($label) a CAS with the wrong expected old fails" test "$FIX2_RC" -ne 0
    assert "($label) the failed CAS leaves main where it was" test "$(fix2_main)" = "$t1"
    assert "($label) the failed CAS consumed nothing: the token is still present" test -e "$F2_SENTINEL"
    in_fix2 sh -c "$unmark_cmd"
    assert "($label) unmark exits 0" test "$FIX2_RC" -eq 0
    assert "($label) unmark after the failed CAS removes the token" bash -c "! test -e '$F2_SENTINEL'"

    printf '%s\n' "$t0" > "$F2_SENTINEL"
    in_fix2 sh -c "$unmark_cmd"
    assert "($label) unmark of a token bound to another tip exits 0" test "$FIX2_RC" -eq 0
    assert "($label) unmark leaves a token bound to another tip in place" test "$(fix2_token)" = "$t0"
}

echo ""
echo "--- (B8 script) scripts/main-gate-mark.sh mark|unmark around a CAS ---"
b8_seam "B8 script" 'scripts/main-gate-mark.sh mark' 'scripts/main-gate-mark.sh unmark'

echo ""
echo "--- (B8 usage) scripts/main-gate-mark.sh argument handling ---"
make_fix2
TIP="$(fix2_main)"
printf '%s\n' "$TIP" > "$F2_SENTINEL"
in_fix2 scripts/main-gate-mark.sh
assert "(B8 usage) no argument -> exit 64" test "$FIX2_RC" -eq 64
in_fix2 scripts/main-gate-mark.sh bogus
assert "(B8 usage) an unknown argument -> exit 64" test "$FIX2_RC" -eq 64
assert "(B8 usage) usage errors leave the token untouched" test "$(fix2_token)" = "$TIP"
in_fix2 scripts/main-gate-mark.sh --help
assert "(B8 usage) --help -> exit 0" test "$FIX2_RC" -eq 0

echo ""
echo "--- (B8 publish) a mark whose rename fails reports it and leaves no temp file ---"
make_fix2
MV_FAILS="$(mktemp -d)"; _TMPDIRS+=("$MV_FAILS")
printf '#!/bin/sh\nexit 1\n' > "$MV_FAILS/mv"; chmod +x "$MV_FAILS/mv"
in_fix2 PATH="$MV_FAILS:$PATH" scripts/main-gate-mark.sh mark
assert "(B8 publish) mark exits non-zero" test "$FIX2_RC" -ne 0
assert "(B8 publish) no sentinel was published" bash -c "! test -e '$F2_SENTINEL'"
assert "(B8 publish) no temp file is left in the common git dir" \
    bash -c "! compgen -G '$F2_SENTINEL.tmp.*' >/dev/null"

echo ""
echo "--- (B8 yaml) dark-factory-orchestrator.yaml git.main_gate_{mark,unmark}_command ---"
if ! python3 -c 'import yaml' 2>/dev/null; then
    echo "SKIP: python3 'yaml' (PyYAML) not available; skipping the B8 yaml face (the script face above still asserts B8)"
else
    yaml_git_key() {
        python3 -c 'import sys,yaml; print(yaml.safe_load(open(sys.argv[1]))["git"][sys.argv[2]])' \
            "$REPO_ROOT/dark-factory-orchestrator.yaml" "$1"
    }
    MARK_CMD="$(yaml_git_key main_gate_mark_command)" || MARK_CMD=""
    UNMARK_CMD="$(yaml_git_key main_gate_unmark_command)" || UNMARK_CMD=""
    assert "(B8 yaml) git.main_gate_mark_command is set" test -n "$MARK_CMD"
    assert "(B8 yaml) git.main_gate_unmark_command is set" test -n "$UNMARK_CMD"
    b8_seam "B8 yaml" "$MARK_CMD" "$UNMARK_CMD"
fi

test_summary
