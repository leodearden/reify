#!/usr/bin/env bash
# tests/infra/test_main_gate_worktree_config.sh
# Tests for scripts/setup-main-gate-worktree-config.sh — the helper that seeds
# per-worktree core.hooksPath via extensions.worktreeConfig so Claude Code's
# plain `git config` write to shared .git/config can no longer darken the gate.
#
# Drives the helper against throwaway git repos; never touches the real repo.
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

HELPER="$REPO_ROOT/scripts/setup-main-gate-worktree-config.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

# ── helpers ───────────────────────────────────────────────────────────────────

# make_repo — create a fresh throwaway git repo; prints its path
# Uses -b main so refs/heads/main exists after the first commit; this is
# required for the G2b genuine-move test (creation events are not gated).
make_repo() {
    local dir
    dir="$(mktemp -d)"; _TMPDIRS+=("$dir")
    git -C "$dir" init -q -b main
    git -C "$dir" config user.email test@test.com
    git -C "$dir" config user.name Test
    echo "$dir"
}

# make_commit DIR [MSG] — add one commit to DIR; prints the new SHA
make_commit() {
    local dir="$1" msg="${2:-commit}"
    printf '%s\n' "$msg" > "$dir/file.txt"
    git -C "$dir" add file.txt
    git -C "$dir" commit -q -m "$msg"
    git -C "$dir" rev-parse HEAD
}

echo "=== main-gate worktree config isolation ==="

# ==============================================================================
# (a) Helper script exists and is executable
# ==============================================================================
echo ""
echo "--- (a) helper exists and is executable ---"

assert "(a) scripts/setup-main-gate-worktree-config.sh exists" \
    test -f "$HELPER"

assert "(a) scripts/setup-main-gate-worktree-config.sh is executable" \
    test -x "$HELPER"

# ==============================================================================
# (b) extensions.worktreeConfig is enabled after running the helper
# ==============================================================================
echo ""
echo "--- (b) extensions.worktreeConfig enabled ---"

REPO_B="$(make_repo)"
"$HELPER" "$REPO_B" >/dev/null 2>&1

assert "(b) extensions.worktreeConfig == true" \
    bash -c "[ \"\$(git -C '$REPO_B' config --get extensions.worktreeConfig 2>/dev/null)\" = 'true' ]"

# ==============================================================================
# (c) core.hooksPath seeded in config.worktree (== hooks)
# ==============================================================================
echo ""
echo "--- (c) core.hooksPath seeded in config.worktree ---"

assert "(c) git config --worktree --get core.hooksPath == hooks" \
    bash -c "[ \"\$(git -C '$REPO_B' config --worktree --get core.hooksPath 2>/dev/null)\" = 'hooks' ]"

assert "(c) .git/config.worktree file contains hooksPath" \
    grep -q 'hooksPath' "$REPO_B/.git/config.worktree"

# ==============================================================================
# (d) G2a: worktree value overrides shared config
# ==============================================================================
echo ""
echo "--- (d) G2a: worktree value overrides shared config ---"

# Write bogus value to shared config (simulating what Claude Code writes)
git -C "$REPO_B" config core.hooksPath /tmp/bogus

assert "(d) effective core.hooksPath is still 'hooks' despite shared /tmp/bogus" \
    bash -c "[ \"\$(git -C '$REPO_B' config --get core.hooksPath 2>/dev/null)\" = 'hooks' ]"

# ==============================================================================
# (e) G2b: end-to-end gate liveness — reference-transaction fires via worktree path
# ==============================================================================
echo ""
echo "--- (e) G2b: end-to-end gate liveness ---"

REPO_E="$(make_repo)"

# Make two commits BEFORE enabling worktreeConfig (default .git/hooks has only
# .sample files — no reference-transaction hook fires during these commits).
C1="$(make_commit "$REPO_E" "first")"
C2="$(make_commit "$REPO_E" "second")"

# Seed the per-worktree config: extensions.worktreeConfig=true + config.worktree
"$HELPER" "$REPO_E" >/dev/null 2>&1

# Copy the real gate hooks into the test repo's hooks/ dir so they can fire.
mkdir -p "$REPO_E/hooks"
cp "$REPO_ROOT/hooks/reference-transaction" "$REPO_E/hooks/"
cp "$REPO_ROOT/hooks/main-gate-lib.sh"      "$REPO_E/hooks/"
chmod +x "$REPO_E/hooks/reference-transaction"

# Sabotage shared config (Claude Code's clobber).
# With extensions.worktreeConfig=true, the effective core.hooksPath remains 'hooks'
# from config.worktree — the shared /tmp/bogus loses the priority battle.
git -C "$REPO_E" config core.hooksPath /tmp/bogus

# Genuine ref move: reset main from C2 to C1 (both non-zero, different).
# git reset --hard supplies the old OID explicitly, which makes git send
# old=C2,new=C1 to the reference-transaction hook (not old=0000 as
# `git update-ref NEW` without explicit old does with extensions.worktreeConfig).
# Git resolves core.hooksPath → 'hooks' (from config.worktree) → $REPO_E/hooks/
# → fires reference-transaction → logs the unsanctioned move.
git -C "$REPO_E" reset --hard "$C1" >/dev/null 2>&1

LOG_E="$REPO_E/.git/reify-main-gate.log"
assert "(e) reference-transaction hook fired and logged 'main move'" \
    bash -c "test -f '$LOG_E' && grep -q 'main move' '$LOG_E'"

# ==============================================================================
# (f) Idempotency — running the helper twice leaves values unchanged
# ==============================================================================
echo ""
echo "--- (f) idempotency ---"

REPO_F="$(make_repo)"
"$HELPER" "$REPO_F" >/dev/null 2>&1
WKTREE_CFG_BEFORE="$(cat "$REPO_F/.git/config.worktree")"

RC_F=0
"$HELPER" "$REPO_F" >/dev/null 2>&1 || RC_F=$?
WKTREE_CFG_AFTER="$(cat "$REPO_F/.git/config.worktree")"

assert "(f) second run exits 0" \
    test "$RC_F" -eq 0

assert "(f) config.worktree unchanged after second run" \
    test "$WKTREE_CFG_BEFORE" = "$WKTREE_CFG_AFTER"

# ==============================================================================
# (g) Linked-worktree no-regression: a linked worktree still inherits shared value
# ==============================================================================
echo ""
echo "--- (g) linked-worktree no-regression ---"

REPO_G="$(make_repo)"
make_commit "$REPO_G" "init" >/dev/null
# Shared value (matches dark-factory's create_worktree write)
git -C "$REPO_G" config core.hooksPath hooks

LINKED_G="$(mktemp -d)"; _TMPDIRS+=("$LINKED_G")
rmdir "$LINKED_G"   # git worktree add requires the target path to not exist
git -C "$REPO_G" worktree add -q "$LINKED_G" HEAD

# Seed only the main worktree — the linked worktree's gitdir gets no config.worktree
"$HELPER" "$REPO_G" >/dev/null 2>&1

# Linked worktree has no per-worktree override → falls back to shared 'hooks'
assert "(g) linked worktree still inherits shared core.hooksPath=hooks" \
    bash -c "[ \"\$(git -C '$LINKED_G' config --get core.hooksPath 2>/dev/null)\" = 'hooks' ]"

# ==============================================================================
# Leak-guard assertions (step-3): helper must abort when core.bare=true or
# core.worktree is set, and must write ONLY core.hooksPath to config.worktree.
# ==============================================================================

# ==============================================================================
# (leak-a) core.bare=true — helper exits non-zero, worktreeConfig NOT enabled
# ==============================================================================
echo ""
echo "--- (leak-a) core.bare=true: helper exits non-zero ---"

REPO_LA="$(make_repo)"
git -C "$REPO_LA" config core.bare true

RC_LA=0
"$HELPER" "$REPO_LA" >/dev/null 2>&1 || RC_LA=$?

assert "(leak-a) bare=true: helper exits non-zero" \
    test "$RC_LA" -ne 0

assert "(leak-a) bare=true: extensions.worktreeConfig NOT enabled" \
    bash -c "val=\$(git -C '$REPO_LA' config --get extensions.worktreeConfig 2>/dev/null || true); [ -z \"\$val\" ] || [ \"\$val\" = 'false' ]"

# ==============================================================================
# (leak-b) core.worktree set — helper exits non-zero
# ==============================================================================
echo ""
echo "--- (leak-b) core.worktree set: helper exits non-zero ---"

REPO_LB="$(make_repo)"
git -C "$REPO_LB" config core.worktree /tmp  # valid path — guards must detect this

RC_LB=0
"$HELPER" "$REPO_LB" >/dev/null 2>&1 || RC_LB=$?

assert "(leak-b) core.worktree set: helper exits non-zero" \
    test "$RC_LB" -ne 0

# ==============================================================================
# (leak-c) nothing-leaks: config.worktree has hooksPath and ONLY hooksPath
# ==============================================================================
echo ""
echo "--- (leak-c) nothing-leaks: config.worktree contains only hooksPath ---"

REPO_LC="$(make_repo)"
"$HELPER" "$REPO_LC" >/dev/null 2>&1

assert "(leak-c) config.worktree contains hooksPath" \
    grep -q 'hooksPath' "$REPO_LC/.git/config.worktree"

assert "(leak-c) config.worktree does NOT contain core.bare" \
    bash -c "! git -C '$REPO_LC' config --worktree --get core.bare >/dev/null 2>&1"

assert "(leak-c) config.worktree does NOT contain core.worktree" \
    bash -c "! git -C '$REPO_LC' config --worktree --get core.worktree >/dev/null 2>&1"

test_summary
