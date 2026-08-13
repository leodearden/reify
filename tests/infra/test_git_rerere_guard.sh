#!/usr/bin/env bash
# tests/infra/test_git_rerere_guard.sh — Tests for scripts/git-rerere-guard.sh,
# the guard that keeps git rerere disabled repo-wide.
#
# WHY THE GUARD EXISTS: `.git/rr-cache` is a git COMMON path (it resolves to the
# common git dir from every linked worktree) while `MERGE_RR` is per-worktree, so
# ~238 warm lanes share ONE unlocked resolution cache. Git takes its only rerere
# lock on the per-worktree MERGE_RR, giving zero cross-worktree exclusion over the
# shared payload directory, and git exposes no knob to relocate rr-cache. See
# docs/notes/git-rerere-shared-worktree-hazard.md (task 5870, esc-5785-5).
#
# Drives the guard against throwaway git repos; never touches the real repo.
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

GUARD="$REPO_ROOT/scripts/git-rerere-guard.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

# ── helpers ───────────────────────────────────────────────────────────────────

# make_repo — create a fresh throwaway git repo with one commit; prints its path.
# -b main so refs/heads/main exists, matching test_main_gate_worktree_config.sh.
#
# rerere.enabled is deliberately NOT set here: git's default is -1 ("enabled iff
# rr-cache/ exists"), and several scenarios below depend on observing that
# unset-vs-explicit-false distinction, so the factory must not pre-decide it.
make_repo() {
    local dir
    dir="$(mktemp -d)"; _TMPDIRS+=("$dir")
    git -C "$dir" init -q -b main
    git -C "$dir" config user.email test@test.com
    git -C "$dir" config user.name Test
    printf 'base\n' > "$dir/file.txt"
    git -C "$dir" add file.txt
    git -C "$dir" commit -q -m base
    echo "$dir"
}

# common_dir DIR — absolute path of DIR's COMMON git dir (where shared config
# and rr-cache live), resolving git's possibly-relative answer.
common_dir() {
    local dir="$1" cd_out
    cd_out="$(git -C "$dir" rev-parse --git-common-dir)"
    case "$cd_out" in
        /*) printf '%s\n' "$cd_out" ;;
        *)  (cd "$dir" && cd "$cd_out" && pwd) ;;
    esac
}

echo "=== git rerere shared-worktree guard ==="

# ==============================================================================
# (a) Guard script exists and is executable
# ==============================================================================
echo ""
echo "--- (a) guard exists and is executable ---"

assert "(a) scripts/git-rerere-guard.sh exists" \
    test -f "$GUARD"

assert "(a) scripts/git-rerere-guard.sh is executable" \
    test -x "$GUARD"

# ==============================================================================
# (b) --help exits 0 and names the three subcommands
# ==============================================================================
echo ""
echo "--- (b) --help names check / arm / scan-locks ---"

assert "(b) --help exits 0" \
    bash "$GUARD" --help

assert "(b) -h exits 0" \
    bash "$GUARD" -h

# grep -w, not a bare substring match: this suite's own REPO_ROOT can live under
# a path containing "warm-lanes", and a bare `grep -q arm` matches the "arm" in
# "warm" — so an absent guard's `bash: <path>: No such file` error would PASS the
# 'arm' assertion. Word-boundary matching keeps the RED honest.
for _sub in check arm scan-locks; do
    assert "(b) --help mentions subcommand '$_sub'" \
        bash -c "bash '$GUARD' --help 2>&1 | grep -qw -- '$_sub'"
done
unset _sub

# ==============================================================================
# (c) Unknown subcommand exits non-zero with usage on stderr
# ==============================================================================
echo ""
echo "--- (c) unknown subcommand is rejected ---"

assert "(c) unknown subcommand exits non-zero" \
    bash -c "! bash '$GUARD' bogus-subcommand >/dev/null 2>&1"

assert "(c) unknown subcommand writes usage to STDERR (not stdout)" \
    bash -c "bash '$GUARD' bogus-subcommand 2>&1 >/dev/null | grep -qi 'usage'"

assert "(c) unknown subcommand names the offending word" \
    bash -c "bash '$GUARD' bogus-subcommand 2>&1 >/dev/null | grep -q 'bogus-subcommand'"

# ==============================================================================
# (d) A bare no-arg invocation is NON-DESTRUCTIVE
#
# The guard's whole point is that it can be run anywhere without side effects
# until `arm` is asked for explicitly. A bare invocation must never write config
# — not to the repo it is invoked from, and not to the user's global config.
# Asserted by byte-comparing the real repo's shared .git/config before and after;
# the guard defaults its target to the repo root one level up from the script, so
# this bare run is exactly the case that would clobber the live store.
# ==============================================================================
echo ""
echo "--- (d) bare no-arg invocation writes no config ---"

_bare_dir="$(mktemp -d)"; _TMPDIRS+=("$_bare_dir")
_cfg_before="$_bare_dir/config.before"
_cfg_after="$_bare_dir/config.after"
_real_common="$(git -C "$REPO_ROOT" rev-parse --git-common-dir)"
case "$_real_common" in
    /*) ;;
    *) _real_common="$REPO_ROOT/$_real_common" ;;
esac

cp "$_real_common/config" "$_cfg_before"
bash "$GUARD" >/dev/null 2>&1 || true
cp "$_real_common/config" "$_cfg_after"

assert "(d) bare invocation leaves shared .git/config byte-identical" \
    cmp -s "$_cfg_before" "$_cfg_after"

unset _bare_dir _cfg_before _cfg_after _real_common

# ==============================================================================
# (e) `check` core — reads the EFFECTIVE rerere.enabled / rerere.autoupdate and
#     exits non-zero when either is armed, naming the offending key.
# ==============================================================================
echo ""
echo "--- (e) check reports effectively-armed rerere ---"

REPO_ON="$(make_repo)"
git -C "$REPO_ON" config rerere.enabled true
git -C "$REPO_ON" config rerere.autoupdate true

assert "(e-a) rerere.enabled=true -> check exits non-zero" \
    bash -c "! bash '$GUARD' check '$REPO_ON' >/dev/null 2>&1"

assert "(e-a) check stderr names rerere.enabled" \
    bash -c "bash '$GUARD' check '$REPO_ON' 2>&1 >/dev/null | grep -q 'rerere.enabled'"

REPO_OFF="$(make_repo)"
git -C "$REPO_OFF" config rerere.enabled false
git -C "$REPO_OFF" config rerere.autoupdate false

assert "(e-b) explicit false/false -> check exits 0" \
    bash "$GUARD" check "$REPO_OFF"

assert "(e-b) check emits NOTHING on stdout when clean" \
    bash -c "[ -z \"\$(bash '$GUARD' check '$REPO_OFF' 2>/dev/null)\" ]"

# autoupdate is the half that turns a bled-in resolution from "visible conflict"
# into "already staged, clean git status", so it must be reported independently
# of rerere.enabled rather than folded into it.
REPO_AU="$(make_repo)"
git -C "$REPO_AU" config rerere.enabled false
git -C "$REPO_AU" config rerere.autoupdate true

assert "(e-c) enabled=false but autoupdate=true -> check exits non-zero" \
    bash -c "! bash '$GUARD' check '$REPO_AU' >/dev/null 2>&1"

assert "(e-c) check stderr names rerere.autoupdate" \
    bash -c "bash '$GUARD' check '$REPO_AU' 2>&1 >/dev/null | grep -q 'rerere.autoupdate'"

# ── (e-d) check is READ-ONLY, including on the failing paths ──────────────────
# A detector that mutates the state it inspects cannot be run safely across 238
# live lanes, so this is asserted on the armed repos too, not just the clean one.
echo ""
echo "--- (e-d) check never writes config ---"

_ro_snap="$(mktemp -d)"; _TMPDIRS+=("$_ro_snap")
_i=0
for _r in "$REPO_ON" "$REPO_OFF" "$REPO_AU"; do
    _i=$((_i + 1))
    _cd="$(common_dir "$_r")"
    cp "$_cd/config" "$_ro_snap/before.$_i"
    bash "$GUARD" check "$_r" >/dev/null 2>&1 || true
    cp "$_cd/config" "$_ro_snap/after.$_i"
    assert "(e-d) check leaves .git/config byte-identical (repo $_i)" \
        cmp -s "$_ro_snap/before.$_i" "$_ro_snap/after.$_i"
done
unset _ro_snap _i _r _cd

test_summary
