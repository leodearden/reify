#!/usr/bin/env bash
# tests/infra/test_git_rerere_guard.sh — Tests for scripts/git-rerere-guard.sh,
# the guard that keeps git rerere disabled repo-wide.
#
# WHY THE GUARD EXISTS: `.git/rr-cache` is a git COMMON path (it resolves to the
# common git dir from every linked worktree) while `MERGE_RR` is per-worktree, so
# every warm lane shares ONE unlocked resolution cache. Git takes its only rerere
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

# HERMETIC GIT CONFIG. Every fixture below is a bare `git init`, which inherits
# the invoking user's ~/.gitconfig and /etc/gitconfig — and `rerere.enabled =
# true` is a very common developer global. That silently poisons every scenario
# that depends on the key being UNSET or effectively false: measured, with a
# global `[rerere] enabled = true / autoupdate = true`, this suite goes from
# 114 passed / 0 failed to 109 / 5 — (f-a) 'unset is genuinely unset', (f-b),
# (f-c), (g-d) and (g-e-f) all turn red, with the paired `arm` asserts. The
# orchestrator host happens to carry no global rerere keys today, so it is green
# here by luck; any developer with rerere on globally would get a confusing
# spurious RED from a `pool`-classified merge-gate test.
#
# EXPORTED, not just set: the assertions below invoke `bash "$GUARD"` as a child
# process, and the guard's own `git config` reads must be isolated too, not only
# the fixture factories'. Same pattern as tests/infra/
# test_harness_baseline_registration_gate.sh.
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_CONFIG_SYSTEM=/dev/null
export GIT_CONFIG_NOSYSTEM=1

_TMPDIRS=()
# chmod before rm: (g-g) chmod 000s a config.worktree and (h-g) drops write
# permission on a whole git dir to force a config-write failure. Both restore
# permissions inline, but an assert that dies mid-block would otherwise leave a
# fixture `rm -rf` cannot reclaim — a silent /tmp leak on a disk-pressured host.
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do
        chmod -R u+rwX "$d" 2>/dev/null || true
        rm -rf "$d"
    done
}
trap cleanup EXIT

# ONE root for every fixture this suite creates, registered here in the PARENT
# shell. The factories below are called as `$(make_repo)` — a COMMAND
# SUBSTITUTION, i.e. a SUBSHELL — so a `_TMPDIRS+=(...)` executed inside them is
# discarded when that subshell exits and the directory leaks forever. Measured:
# repeated runs had accreted ~100 abandoned git repos and worktrees under /tmp,
# which is exactly the wrong behaviour for a `pool`-classified test that the
# merge gate re-runs constantly on a disk-pressured host. Rooting every fixture
# under one parent-registered directory sidesteps the subshell problem entirely:
# cleanup removes the root, so nothing needs per-path registration.
_SUITE_TMP="$(mktemp -d)"; _TMPDIRS+=("$_SUITE_TMP")

# ── helpers ───────────────────────────────────────────────────────────────────

# make_repo — create a fresh throwaway git repo with one commit; prints its path.
# -b main so refs/heads/main exists, matching test_main_gate_worktree_config.sh.
#
# rerere.enabled is deliberately NOT set here: git's default is -1 ("enabled iff
# rr-cache/ exists"), and several scenarios below depend on observing that
# unset-vs-explicit-false distinction, so the factory must not pre-decide it.
make_repo() {
    local dir
    # Under $_SUITE_TMP, NOT a bare `mktemp -d`: see the note above — this runs
    # in a subshell, so registering the path for cleanup here would be a no-op.
    dir="$(mktemp -d "$_SUITE_TMP/repo.XXXXXX")"
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
# (d) The DEFAULT-TARGET path is non-destructive
#
# The guard's whole point is that it can be run anywhere without side effects
# until `arm` is asked for explicitly. Exercised against a COPY of the guard
# installed at <throwaway>/scripts/, so that the default target (the repo root
# one level up from the script) resolves to a store this suite owns.
#
# HERMETIC ON PURPOSE. An earlier version snapshotted the LIVE shared
# .git/config here, which was wrong twice over. (1) Non-hermetic: this suite is
# classified `pool` (run-all-classification.manifest) — sibling lanes run
# setup-dev.sh concurrently and Claude Code rewrites that shared file on every
# worktree enter, so a foreign write landing between the two snapshots was a
# spurious merge-gate FAIL. (2) Vacuous: a bare no-arg run exits at the
# missing-subcommand branch BEFORE target resolution or any git call, so the
# byte compare could never fail from guard behaviour. Drive `check` — the code
# path that actually resolves a target — and keep the real store out of it.
# ==============================================================================
echo ""
echo "--- (d) default-target resolution writes no config ---"

_dflt_repo="$(make_repo)"
mkdir -p "$_dflt_repo/scripts"
cp "$GUARD" "$_dflt_repo/scripts/git-rerere-guard.sh"
_dflt_guard="$_dflt_repo/scripts/git-rerere-guard.sh"
# Armed, so the default-target run reaches a real verdict instead of passing
# because it happened to inspect nothing.
git -C "$_dflt_repo" config rerere.enabled true
_dflt_cd="$(common_dir "$_dflt_repo")"
_dflt_snap="$(mktemp -d "$_SUITE_TMP/snap.XXXXXX")"
cp "$_dflt_cd/config" "$_dflt_snap/before"

assert "(d) a bare no-arg invocation exits non-zero" \
    bash -c "! bash '$_dflt_guard' >/dev/null 2>&1"

assert "(d) check with NO target_dir still reaches a verdict (default target resolved)" \
    bash -c "! bash '$_dflt_guard' check >/dev/null 2>&1"

# Proves WHICH store the default resolved to: the guard prints the common dir it
# judged, so this fails if the default ever drifts to some other repo.
assert "(d) ...and the verdict names the store one level up from the script" \
    bash -c "bash '$_dflt_guard' check 2>&1 >/dev/null | grep -q '$_dflt_cd'"

cp "$_dflt_cd/config" "$_dflt_snap/after"

assert "(d) neither invocation wrote to the defaulted store's config" \
    cmp -s "$_dflt_snap/before" "$_dflt_snap/after"

unset _dflt_repo _dflt_guard _dflt_cd _dflt_snap

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
# A detector that mutates the state it inspects cannot be run safely across every
# live lane of the store, so this is asserted on the armed repos too, not just the
# clean one.
echo ""
echo "--- (e-d) check never writes config ---"

_ro_snap="$(mktemp -d "$_SUITE_TMP/snap.XXXXXX")"
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

# ==============================================================================
# (f) M4 — THE IMPLICIT RE-ARM.  git's default for rerere.enabled is -1, meaning
#     "enabled iff rr-cache/ exists".  With the key UNSET and the residual
#     rr-cache/ still on disk, rerere is silently ON for the whole fleet — so
#     `git config --unset rerere.enabled` is a silent RE-ARM, not a no-op.
#     This is the subtlest behaviour in the guard and the reason it ships as a
#     re-runnable check rather than a one-shot config write.
# ==============================================================================
echo ""
echo "--- (f) unset rerere.enabled + residual rr-cache/ = silently armed ---"

# make_conflict DIR — build a conflicting side branch and leave DIR back on its
# ORIGINAL branch, ready for `git merge <side>` to conflict.  Prints the side
# branch's name.
#
# BRANCH-AGNOSTIC ON PURPOSE.  This is called both on a main-checkout fixture and
# inside a LINKED WORKTREE, and a hardcoded `git checkout main` fails there with
# "fatal: 'main' is already used by worktree at ..." — which under this suite's
# `set -e` aborts the whole run rather than failing one assert.  Deriving the
# base branch from HEAD, and naming the side branch after it, also keeps two
# worktrees of the SAME store from colliding on one branch name.
make_conflict() {
    local dir="$1" base side
    base="$(git -C "$dir" rev-parse --abbrev-ref HEAD)"
    side="side-$base"
    git -C "$dir" checkout -q -b "$side"
    printf 'side\n' > "$dir/file.txt"
    git -C "$dir" add file.txt
    git -C "$dir" commit -q -m side
    git -C "$dir" checkout -q "$base"
    printf 'ours\n' > "$dir/file.txt"
    git -C "$dir" add file.txt
    git -C "$dir" commit -q -m ours
    printf '%s\n' "$side"
}

# count_rr_entries DIR — number of rr-cache/<id>/ entries in DIR's common store.
count_rr_entries() {
    local cd_path
    cd_path="$(common_dir "$1")"
    find "$cd_path/rr-cache" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l
}

# (f-a) unset + rr-cache/ present -> armed
REPO_IMPLICIT="$(make_repo)"
mkdir -p "$(common_dir "$REPO_IMPLICIT")/rr-cache"

assert "(f-a) rerere.enabled unset is genuinely unset in the fixture" \
    bash -c "! git -C '$REPO_IMPLICIT' config --get rerere.enabled >/dev/null 2>&1"

assert "(f-a) unset + rr-cache/ present -> check exits non-zero" \
    bash -c "! bash '$GUARD' check '$REPO_IMPLICIT' >/dev/null 2>&1"

# Pins WHICH branch fired — the implicit re-arm (unset key + residual cache),
# not the plain enabled=true branch — since only the former justifies leaving the
# cache in place. 'UNSET' is a token the guard's interpolated paths cannot
# supply. Two earlier assertions here were tautological and are deliberately
# gone: `grep -q 'rr-cache'` held because the ARMED line interpolates $RR_CACHE,
# whose path always ends in /rr-cache, and `grep -q -- '-1'` was a bare substring
# pin on prose that the same interpolated path could satisfy. The exit-code
# assertion above is the behavioural contract.
assert "(f-a) check stderr identifies the UNSET-key branch, not plain enabled=true" \
    bash -c "bash '$GUARD' check '$REPO_IMPLICIT' 2>&1 >/dev/null | grep -q 'UNSET'"

# (f-b) unset + NO rr-cache/ -> safe
REPO_NORR="$(make_repo)"

assert "(f-b) fixture really has no rr-cache/ directory" \
    bash -c "! test -d '$(common_dir "$REPO_NORR")/rr-cache'"

assert "(f-b) unset + no rr-cache/ -> check exits 0" \
    bash "$GUARD" check "$REPO_NORR"

# (f-c) EXPLICIT false + rr-cache/ present -> safe.  This is the load-bearing
# scenario: it is what lets `arm` leave the residual rr-cache in place
# instead of pruning it, and pruning is precisely the operation that reproduces
# the segfault + stale-lock signature across live lanes.
REPO_EXPLICIT="$(make_repo)"
mkdir -p "$(common_dir "$REPO_EXPLICIT")/rr-cache"
git -C "$REPO_EXPLICIT" config rerere.enabled false

assert "(f-c) explicit false + rr-cache/ present -> check exits 0" \
    bash "$GUARD" check "$REPO_EXPLICIT"

# ── (f-d) BEHAVIOURAL ORACLE ──────────────────────────────────────────────────
# Measures git's ACTUAL behaviour rather than trusting the config read: with the
# explicit false and a residual rr-cache/ present, a real conflicted merge must
# record ZERO new rr-cache/<id>/ entries.  Without this, the whole suite would
# only be asserting that the guard agrees with itself.
echo ""
echo "--- (f-d) behavioural oracle: explicit false records no rr-cache entries ---"

_side="$(make_conflict "$REPO_EXPLICIT")"
_rr_before="$(count_rr_entries "$REPO_EXPLICIT")"
git -C "$REPO_EXPLICIT" merge "$_side" >/dev/null 2>&1 || true
_rr_after="$(count_rr_entries "$REPO_EXPLICIT")"

assert "(f-d) the merge really did conflict (fixture is live, not vacuous)" \
    bash -c "git -C '$REPO_EXPLICIT' ls-files -u | grep -q ."

assert "(f-d) explicit false -> conflicted merge records ZERO new rr-cache entries" \
    test "$_rr_before" -eq "$_rr_after"

git -C "$REPO_EXPLICIT" merge --abort >/dev/null 2>&1 || true
unset _rr_before _rr_after _side

# ── (f-e) POSITIVE CONTROL for the oracles ────────────────────────────────────
# (f-d) above and (h-e) below measure only the NEGATIVE direction — "with rerere
# off, zero new rr-cache entries". If count_rr_entries or make_conflict ever
# regressed into observing nothing at all (wrong common dir, a merge that never
# reaches rerere, a fixture that stopped conflicting), BOTH would pass vacuously
# and the suite would report green while measuring nothing. So prove the
# instrument can register a non-zero delta: the same fixture shape with rerere
# left implicitly ON must record exactly one entry. Same intent as
# assert_shared_trash_litter_detector_live in test_helpers.sh — a checker has to
# be shown to fire before its silence means anything.
echo ""
echo "--- (f-e) positive control: the oracle CAN observe a non-zero delta ---"

REPO_POS="$(make_repo)"
mkdir -p "$(common_dir "$REPO_POS")/rr-cache"   # unset + rr-cache present = ON

_pos_side="$(make_conflict "$REPO_POS")"
_pos_before="$(count_rr_entries "$REPO_POS")"
git -C "$REPO_POS" merge "$_pos_side" >/dev/null 2>&1 || true
_pos_after="$(count_rr_entries "$REPO_POS")"

assert "(f-e) the control merge really did conflict (control is live)" \
    bash -c "git -C '$REPO_POS' ls-files -u | grep -q ."

assert "(f-e) implicitly-armed rerere records EXACTLY ONE new rr-cache entry" \
    test "$_pos_after" -eq "$((_pos_before + 1))"

git -C "$REPO_POS" merge --abort >/dev/null 2>&1 || true
unset _pos_before _pos_after _pos_side

# ==============================================================================
# (g) M6 — PER-WORKTREE OVERRIDES BEAT SHARED CONFIG.  extensions.worktreeConfig
#     is already true on the live repo and every worktree carries a
#     config.worktree, and git reads config.worktree FIRST.  So a single lane can
#     re-arm rerere for itself while the shared .git/config still reads false —
#     `check` must sweep the linked worktrees, not just the shared file.
# ==============================================================================
echo ""
echo "--- (g) check sweeps per-worktree config.worktree overrides ---"

# make_wt_repo — a repo with extensions.worktreeConfig on, shared rerere off, and
# two linked worktrees.  Prints "<repo> <wtA> <wtB>".
make_wt_repo() {
    local dir wt_a wt_b
    dir="$(make_repo)"
    git -C "$dir" config extensions.worktreeConfig true
    git -C "$dir" config rerere.enabled false
    git -C "$dir" config rerere.autoupdate false
    # Siblings of $dir, so they land under $_SUITE_TMP and the root sweep in
    # cleanup reclaims them. Registering them here would not work: this factory
    # is called via command substitution and runs in a subshell.
    wt_a="$dir-wtA"; wt_b="$dir-wtB"
    git -C "$dir" worktree add -q -b wtA "$wt_a" >/dev/null 2>&1
    git -C "$dir" worktree add -q -b wtB "$wt_b" >/dev/null 2>&1
    echo "$dir $wt_a $wt_b"
}

read -r WT_REPO WT_A WT_B <<< "$(make_wt_repo)"

assert "(g) fixture: extensions.worktreeConfig is on" \
    bash -c "[ \"\$(git -C '$WT_REPO' config --bool --get extensions.worktreeConfig)\" = true ]"

assert "(g) fixture: both linked worktrees exist" \
    bash -c "test -d '$WT_A' && test -d '$WT_B'"

# (g-b) no override anywhere -> clean.  Asserted BEFORE planting the override so
# it cannot pass merely because the sweep is a no-op on this fixture shape.
assert "(g-b) shared false + no per-worktree override -> check exits 0" \
    bash "$GUARD" check "$WT_REPO"

# Prove config.worktree really does beat the shared false, so the scenario below
# is a genuine hazard and not an artefact of the fixture.
git -C "$WT_A" config --worktree rerere.enabled true

assert "(g) config.worktree BEATS shared config (effective value is true in wtA)" \
    bash -c "[ \"\$(git -C '$WT_A' config --bool --get rerere.enabled)\" = true ]"

assert "(g) ...while the shared config still reads false" \
    bash -c "[ \"\$(git -C '$WT_REPO' config --bool --get rerere.enabled)\" = false ]"

# (g-a) the override is detected, and the offending worktree is named.
assert "(g-a) per-worktree override -> check exits non-zero" \
    bash -c "! bash '$GUARD' check '$WT_REPO' >/dev/null 2>&1"

assert "(g-a) check stderr names the offending worktree" \
    bash -c "bash '$GUARD' check '$WT_REPO' 2>&1 >/dev/null | grep -q 'wtA'"

assert "(g-a) check stderr does NOT name the innocent worktree" \
    bash -c "! bash '$GUARD' check '$WT_REPO' 2>&1 >/dev/null | grep -q 'wtB'"

# (g-c) same verdict from INSIDE a linked worktree — guards the
# --git-common-dir resolution.  Run from wtB (the INNOCENT lane): the offending
# override lives in wtA, so a guard that inspected only its own worktree's
# config would wrongly report clean here.
assert "(g-c) check from inside a linked worktree reaches the same verdict" \
    bash -c "! bash '$GUARD' check '$WT_B' >/dev/null 2>&1"

assert "(g-c) ...and still names the offending worktree" \
    bash -c "bash '$GUARD' check '$WT_B' 2>&1 >/dev/null | grep -q 'wtA'"

# ── (g-d) EXTENSION OFF: git ignores config.worktree ENTIRELY ─────────────────
# The sweep's premise — "config.worktree beats shared config" — holds only while
# extensions.worktreeConfig is true. With the extension off (or removed), git
# never reads config.worktree at all, so a planted rerere.enabled=true is dead
# bytes. Reporting it ARMED is a FALSE verdict, and a uniquely bad one: `arm`
# writes --local only and cannot clear a per-worktree file, so the false ARMED
# would make `arm` fail permanently, and setup-dev.sh's `set -e` would abort the
# rest of the developer setup over a store where nothing is wrong. Every other
# fixture here sets the extension on, so this precondition was never exercised.
echo ""
echo "--- (g-d) config.worktree with extensions.worktreeConfig OFF is inert ---"

NOEXT_REPO="$(make_repo)"
git -C "$NOEXT_REPO" config rerere.enabled false
git -C "$NOEXT_REPO" config rerere.autoupdate false
NOEXT_WT="$NOEXT_REPO-noextwt"; _TMPDIRS+=("$NOEXT_WT")
git -C "$NOEXT_REPO" worktree add -q -b noextwt "$NOEXT_WT" >/dev/null 2>&1
# Planted with `git config --file`, never `--worktree`: git REFUSES the latter
# outright while the extension is off, which is itself the point being pinned.
NOEXT_WT_GITDIR="$(git -C "$NOEXT_WT" rev-parse --absolute-git-dir)"
git config --file "$NOEXT_WT_GITDIR/config.worktree" rerere.enabled true

assert "(g-d) fixture: extensions.worktreeConfig is NOT enabled" \
    bash -c "[ \"\$(git -C '$NOEXT_REPO' config --bool --get extensions.worktreeConfig 2>/dev/null || true)\" != true ]"

assert "(g-d) fixture: the config.worktree really was planted" \
    bash -c "[ \"\$(git config --file '$NOEXT_WT_GITDIR/config.worktree' --bool --get rerere.enabled)\" = true ]"

# The behavioural fact the guard must agree with, measured rather than assumed.
assert "(g-d) git IGNORES the plant — effective rerere.enabled in that worktree is false" \
    bash -c "[ \"\$(git -C '$NOEXT_WT' config --bool --get rerere.enabled)\" = false ]"

assert "(g-d) check does NOT report an inert config.worktree as armed" \
    bash "$GUARD" check "$NOEXT_REPO"

assert "(g-d) ...and arm therefore succeeds on such a store" \
    bash "$GUARD" arm "$NOEXT_REPO"

# ── (g-e) THE MAIN CHECKOUT's own config.worktree is a sweep blind spot ───────
# With extensions.worktreeConfig on, the main checkout's per-worktree config is
# <common>/config.worktree — NOT <common>/worktrees/*/config.worktree — because
# for the main checkout git dir == common dir.  That is the same asymmetry (i-f)
# already pins for scan-locks.  A main-checkout self-arm is therefore invisible
# to BOTH detection paths at once: the effective-value read
# (`git -C $TARGET config --get`) sees only the TARGET's own config.worktree, and
# the sweep globs only the linked worktrees.
#
# MEASURED against the pre-fix guard: shared rerere.enabled=false plus a main-dir
# plant of true made `check <lane>` exit 0 emitting NOTHING while `check <main>`
# exited 1, and `arm <lane>` returned 0 ("disarmed and verified") with main still
# effectively true.  That is a silent FALSE CLEAN — not even the advisory exit 2
# — at the site that matters most: scripts/land.sh runs a real
# `git merge --no-ff` in the main checkout (CLAUDE.md "Landing on main").
echo ""
echo "--- (g-e) check sweeps the main checkout's own config.worktree ---"

read -r GE_REPO GE_A GE_B <<< "$(make_wt_repo)"
GE_COMMON="$(common_dir "$GE_REPO")"

# (g-e-a) NEGATIVE CONTROL FIRST.  With no main-dir plant this fixture is clean,
# so a PASS below cannot be the new sweep arm firing unconditionally.
assert "(g-e-a) no main-dir plant -> check from a lane exits 0" \
    bash "$GUARD" check "$GE_A"

git -C "$GE_REPO" config --worktree rerere.enabled true

# (g-e-b) Fixture preconditions MEASURED, not assumed.
assert "(g-e-b) fixture: the plant landed at <common>/config.worktree" \
    bash -c "[ \"\$(git config --file '$GE_COMMON/config.worktree' --bool --get rerere.enabled)\" = true ]"

assert "(g-e-b) fixture: nothing under worktrees/ carries it — the glob is blind" \
    bash -c "! grep -rqs rerere '$GE_COMMON/worktrees/'"

assert "(g-e-b) fixture: effective rerere.enabled is true in the MAIN checkout" \
    bash -c "[ \"\$(git -C '$GE_REPO' config --bool --get rerere.enabled)\" = true ]"

assert "(g-e-b) fixture: ...while a LANE still reads false (so only the sweep can see it)" \
    bash -c "[ \"\$(git -C '$GE_A' config --bool --get rerere.enabled)\" = false ]"

# (g-e-c) The hazard itself: a lane must now SEE the main checkout's self-arm.
assert "(g-e-c) main-dir override -> check from a lane exits non-zero" \
    bash -c "! bash '$GUARD' check '$GE_A' >/dev/null 2>&1"

assert "(g-e-c) check names the hit '<main checkout>'" \
    bash -c "bash '$GUARD' check '$GE_A' 2>&1 >/dev/null | grep -q -- '<main checkout>'"

# The label must be STATED, not derived: basename(dirname <common>/config.worktree)
# is the useless '.git'.  _classify_lock already labels that same dir
# '<main checkout>' in scan-locks, and the two subcommands must agree.
assert "(g-e-c) ...and never labels it the useless '.git'" \
    bash -c "! bash '$GUARD' check '$GE_A' 2>&1 >/dev/null | grep -q \"worktree '.git'\""

assert "(g-e-c) ...and does not name an innocent linked worktree" \
    bash -c "! bash '$GUARD' check '$GE_A' 2>&1 >/dev/null | grep -q 'wtA'"

# (g-e-d) Main/lane symmetry — the guard's header promises the main checkout and
# any linked lane are interchangeable as targets.
assert "(g-e-d) check from the MAIN checkout also exits non-zero" \
    bash -c "! bash '$GUARD' check '$GE_REPO' >/dev/null 2>&1"

# (g-e-e) `arm` writes --local only and can never clear a per-worktree file, so
# the correct verdict is the advisory 2 — the same contract as (h-f), which
# setup-dev.sh warns-and-continues on.  The shared config is armed FIRST so the
# "still wrote both keys" assertions below are not vacuous.
git -C "$GE_REPO" config rerere.enabled true
git -C "$GE_REPO" config rerere.autoupdate true

bash "$GUARD" arm "$GE_A" >/dev/null 2>&1 && _ge_status=0 || _ge_status=$?

assert "(g-e-e) arm from a lane exits 2 (not 0, not 1) with main self-armed" \
    test "$_ge_status" -eq 2

assert "(g-e-e) arm still wrote rerere.enabled=false to the shared config" \
    bash -c "[ \"\$(git -C '$GE_REPO' config --local --bool --get rerere.enabled)\" = false ]"

assert "(g-e-e) arm still wrote rerere.autoupdate=false to the shared config" \
    bash -c "[ \"\$(git -C '$GE_REPO' config --local --bool --get rerere.autoupdate)\" = false ]"

assert "(g-e-e) arm names '<main checkout>' so an operator can act" \
    bash -c "bash '$GUARD' arm '$GE_A' 2>&1 >/dev/null | grep -q -- '<main checkout>'"

unset _ge_status

# ── (g-e-f) EXTENSION OFF: a main-dir config.worktree is inert too ────────────
# The (g-d) false-positive rule applies identically to the new path.  With
# extensions.worktreeConfig off git never reads config.worktree at all, so a
# main-dir plant is dead bytes; reporting it ARMED would make `arm` fail
# permanently on a healthy store, since `arm` writes --local and can never clear
# a per-worktree file.
echo ""
echo "--- (g-e-f) main-dir config.worktree with the extension OFF is inert ---"

GENX_REPO="$(make_repo)"
GENX_COMMON="$(common_dir "$GENX_REPO")"
git -C "$GENX_REPO" config rerere.enabled false
git -C "$GENX_REPO" config rerere.autoupdate false
# Planted with `git config --file`, never `--worktree`: git REFUSES the latter
# outright while the extension is off, which is itself part of the point.
git config --file "$GENX_COMMON/config.worktree" rerere.enabled true

assert "(g-e-f) fixture: extensions.worktreeConfig is NOT enabled" \
    bash -c "[ \"\$(git -C '$GENX_REPO' config --bool --get extensions.worktreeConfig 2>/dev/null || true)\" != true ]"

assert "(g-e-f) fixture: the main-dir config.worktree really was planted" \
    bash -c "[ \"\$(git config --file '$GENX_COMMON/config.worktree' --bool --get rerere.enabled)\" = true ]"

assert "(g-e-f) git IGNORES it — effective rerere.enabled in the main checkout is false" \
    bash -c "[ \"\$(git -C '$GENX_REPO' config --bool --get rerere.enabled)\" = false ]"

assert "(g-e-f) check does NOT report an inert main-dir config.worktree" \
    bash "$GUARD" check "$GENX_REPO"

assert "(g-e-f) ...and arm therefore succeeds on such a store" \
    bash "$GUARD" arm "$GENX_REPO"

# ── (g-e-g) NO LINKED WORKTREES: the main dir is still swept ──────────────────
# A store with no worktrees/ dir is a normal shape, not a reason to skip the
# sweep — the same reasoning (i-f) already pins for scan-locks.  The load-bearing
# assertion is the '<main checkout>' LABEL: an armed rerere.enabled is also
# visible to `check`'s ordinary effective-value read here (the target IS the main
# checkout), so only the label proves the sweep itself ran on this shape.
echo ""
echo "--- (g-e-g) the sweep reaches the main dir on a store with no lanes ---"

GENW_REPO="$(make_repo)"
GENW_COMMON="$(common_dir "$GENW_REPO")"
git -C "$GENW_REPO" config extensions.worktreeConfig true
git -C "$GENW_REPO" config rerere.enabled false
git -C "$GENW_REPO" config rerere.autoupdate false

assert "(g-e-g) fixture: the store really has no worktrees/ dir" \
    bash -c "! test -d '$GENW_COMMON/worktrees'"

assert "(g-e-g) baseline: clean before the plant" \
    bash "$GUARD" check "$GENW_REPO"

git -C "$GENW_REPO" config --worktree rerere.enabled true

assert "(g-e-g) a main-dir plant is reported even with no worktrees/ dir" \
    bash -c "! bash '$GUARD' check '$GENW_REPO' >/dev/null 2>&1"

assert "(g-e-g) ...naming '<main checkout>', proving the sweep ran on this shape" \
    bash -c "bash '$GUARD' check '$GENW_REPO' 2>&1 >/dev/null | grep -q -- '<main checkout>'"


# ── (g-f) THE SHARED DEFAULT, masked by the TARGET's own config.worktree ──────
# The polarity mirror of (g-e), and the same class of silent false-clean. `check`
# read the master switch as the EFFECTIVE value for $TARGET, and the sweep only
# ever reports config.worktree values that are TRUE — so a lane that disarms
# ITSELF read clean while the shared config still armed every OTHER lane of the
# store.
#
# MEASURED against the pre-fix guard on a throwaway store: shared
# rerere.enabled=true + rerere.autoupdate=true, with one lane setting both false
# in its own config.worktree, made `git-rerere-guard.sh check <lane>` exit 0
# printing NOTHING. It matters because the guard's header promises the main
# checkout and any linked lane are interchangeable as targets, and because
# setup-dev.sh runs the guard from whatever worktree a developer happens to be
# in: one self-disarmed lane would report the whole fleet healthy.
#
# Unlike the inert config.worktree of (g-d), reporting this is SAFE by
# construction — `arm` is a --local writer, so the scope it reports is exactly
# the scope `arm` can clear. (g-f-f) pins that self-healing.
echo ""
echo "--- (g-f) check reports an armed SHARED default that the target masks ---"

read -r GF_REPO GF_A GF_B <<< "$(make_wt_repo)"

# (g-f-a) NEGATIVE CONTROL FIRST. make_wt_repo leaves the shared config explicitly
# false; a lane that redundantly repeats that must stay clean, so nothing below
# can pass merely because the new shared-scope read fires unconditionally.
git -C "$GF_A" config --worktree rerere.enabled false
git -C "$GF_A" config --worktree rerere.autoupdate false

assert "(g-f-a) shared false + lane self-disarm -> check from the lane exits 0" \
    bash "$GUARD" check "$GF_A"

# Now arm the SHARED config while the lane keeps masking it for itself.
git -C "$GF_REPO" config rerere.enabled true
git -C "$GF_REPO" config rerere.autoupdate true

# (g-f-b) Fixture preconditions MEASURED, not assumed.
assert "(g-f-b) fixture: the SHARED config really reads true" \
    bash -c "[ \"\$(git -C '$GF_REPO' config --local --bool --get rerere.enabled)\" = true ]"

assert "(g-f-b) fixture: ...while the lane's EFFECTIVE value is false (the mask)" \
    bash -c "[ \"\$(git -C '$GF_A' config --bool --get rerere.enabled)\" = false ]"

assert "(g-f-b) fixture: ...and the sweep cannot see it — the mask's value is false" \
    bash -c "[ \"\$(git -C '$GF_A' config --worktree --bool --get rerere.autoupdate)\" = false ]"

# (g-f-c) The hazard itself.
assert "(g-f-c) armed shared default masked by the target -> check exits non-zero" \
    bash -c "! bash '$GUARD' check '$GF_A' >/dev/null 2>&1"

assert "(g-f-c) check names the SHARED config as the armed scope" \
    bash -c "bash '$GUARD' check '$GF_A' 2>&1 >/dev/null | grep -qF 'SHARED config sets rerere.enabled=true'"

assert "(g-f-c) ...and reports autoupdate independently, not folded into enabled" \
    bash -c "bash '$GUARD' check '$GF_A' 2>&1 >/dev/null | grep -qF 'SHARED config sets rerere.autoupdate=true'"

assert "(g-f-c) ...and does not misattribute it to an innocent linked worktree" \
    bash -c "! bash '$GUARD' check '$GF_A' 2>&1 >/dev/null | grep -q \"worktree '.*wtB'\""

# (g-f-d) Main/lane symmetry — the same header promise (g-e-d) pins from the
# other side.
assert "(g-f-d) check from the MAIN checkout reaches the same verdict" \
    bash -c "! bash '$GUARD' check '$GF_REPO' >/dev/null 2>&1"

# (g-f-e) NO DOUBLE REPORT. On a store armed OUTRIGHT (nothing masking it) the
# effective read already fires, so the shared-scope read must stay silent — else
# every ordinary armed store would print each key twice.
assert "(g-f-e) an outright-armed store reports rerere.enabled exactly once" \
    bash -c "[ \"\$(bash '$GUARD' check '$GF_REPO' 2>&1 >/dev/null | grep -c '^ARMED:.*rerere\.enabled')\" -eq 1 ]"

# (g-f-f) arm SELF-HEALS this case: it writes --local, which IS the armed scope.
# Exit 0, not the advisory 2 — nothing out of reach survives here.
bash "$GUARD" arm "$GF_A" >/dev/null 2>&1 && _gf_status=0 || _gf_status=$?

assert "(g-f-f) arm from the masking lane exits 0 (self-healed, not advisory 2)" \
    test "$_gf_status" -eq 0

assert "(g-f-f) ...and the shared config now reads false" \
    bash -c "[ \"\$(git -C '$GF_REPO' config --local --bool --get rerere.enabled)\" = false ]"

unset _gf_status

# ── (g-f-g) the -1 default is a SHARED-scope hazard too ───────────────────────
# Same masking shape, but with the shared key UNSET rather than true: git's -1
# default ("enabled iff rr-cache/ exists") arms every lane that does not mask it.
# This is the (f) hazard reached through the (g-f) blind spot, and it is the case
# the fleet is actually in — the residual rr-cache is on disk right now.
echo ""
echo "--- (g-f-g) an UNSET shared key + residual rr-cache is reported too ---"

read -r GFU_REPO GFU_A GFU_B <<< "$(make_wt_repo)"
GFU_COMMON="$(common_dir "$GFU_REPO")"
git -C "$GFU_REPO" config --unset rerere.enabled
git -C "$GFU_REPO" config --unset rerere.autoupdate
git -C "$GFU_A" config --worktree rerere.enabled false
git -C "$GFU_A" config --worktree rerere.autoupdate false

assert "(g-f-g) fixture: the shared key really is unset" \
    bash -c "! git -C '$GFU_REPO' config --local --get rerere.enabled >/dev/null 2>&1"

# NEGATIVE CONTROL: unset with NO rr-cache/ is genuinely safe — the -1 default
# resolves to OFF — so the verdict below must be the CACHE, not the unset key.
assert "(g-f-g) baseline: unset shared key + no rr-cache -> check exits 0" \
    bash "$GUARD" check "$GFU_A"

mkdir -p "$GFU_COMMON/rr-cache/cccc3333"

assert "(g-f-g) unset shared key + residual rr-cache -> check exits non-zero" \
    bash -c "! bash '$GUARD' check '$GFU_A' >/dev/null 2>&1"

assert "(g-f-g) ...and the message names the SHARED scope and the -1 default" \
    bash -c "bash '$GUARD' check '$GFU_A' 2>&1 >/dev/null | grep -qF 'SHARED config leaves rerere.enabled UNSET'"

# rerere.autoupdate defaults to FALSE, so an unset shared autoupdate is genuinely
# safe: the -1 default belongs to rerere.enabled alone and must not be applied to
# both keys by a copy-paste.
assert "(g-f-g) an unset shared rerere.autoupdate is NOT reported" \
    bash -c "! bash '$GUARD' check '$GFU_A' 2>&1 >/dev/null | grep -qF 'SHARED config leaves rerere.autoupdate'"

# ── (g-g) an UNREADABLE config.worktree must not mask the rest of the sweep ───
# The sweep's `[ ! -r ]` WARNING-and-continue arm exists so that one lane whose
# config.worktree cannot be read does not abort the loop and silently declare
# every LATER lane clean. It had no test at all.
#
# The glob is lexical, so chmod-000ing wtA's file and arming wtB puts the
# unreadable file strictly BEFORE the armed one: a sweep that aborted on the
# first would report this store clean. The ordering is asserted on the OUTPUT
# (WARNING line precedes the ARMED line) rather than on the paths, so it pins the
# traversal itself rather than restating the filesystem.
echo ""
echo "--- (g-g) an unreadable config.worktree WARNs and the sweep continues ---"

if [ "$(id -u)" -eq 0 ]; then
    echo "  SKIP: (g-g) running as root — chmod 000 does not deny root reads"
else
    read -r GG_REPO GG_A GG_B <<< "$(make_wt_repo)"
    GG_A_GITDIR="$(git -C "$GG_A" rev-parse --absolute-git-dir)"

    # wtA gets a real config.worktree with NO rerere key at all, so if it were
    # readable it would contribute nothing — the only thing under test is the
    # unreadable branch. wtB is genuinely armed and sorts after it.
    git -C "$GG_A" config --worktree core.hooksPath /dev/null
    git -C "$GG_B" config --worktree rerere.enabled true

    chmod 000 "$GG_A_GITDIR/config.worktree"

    assert "(g-g) fixture: the planted config.worktree really is unreadable" \
        bash -c "! head -c1 '$GG_A_GITDIR/config.worktree' >/dev/null 2>&1"

    assert "(g-g) check WARNs, naming the unreadable file" \
        bash -c "bash '$GUARD' check '$GG_REPO' 2>&1 >/dev/null | grep -qF 'WARNING: cannot read $GG_A_GITDIR/config.worktree'"

    assert "(g-g) ...reporting that lane as UNKNOWN rather than verified safe" \
        bash -c "bash '$GUARD' check '$GG_REPO' 2>&1 >/dev/null | grep -qF 'UNKNOWN, not verified safe'"

    assert "(g-g) the LATER armed worktree is still reported (the sweep continued)" \
        bash -c "bash '$GUARD' check '$GG_REPO' 2>&1 >/dev/null | grep -q \"ARMED: worktree '.*wtB'\""

    assert "(g-g) ...and the WARNING precedes it, so the skip really was traversed past" \
        bash -c "bash '$GUARD' check '$GG_REPO' 2>&1 >/dev/null | awk '/WARNING: cannot read/{w=NR} /ARMED: worktree/{a=NR} END{exit !(w && a && w < a)}'"

    assert "(g-g) ...and the verdict is non-zero" \
        bash -c "! bash '$GUARD' check '$GG_REPO' >/dev/null 2>&1"

    chmod 644 "$GG_A_GITDIR/config.worktree"
    unset GG_REPO GG_A GG_B GG_A_GITDIR
fi

# ── (g-h) THE include.path BLIND SPOT ─────────────────────────────────────────
# `--includes` defaults OFF whenever a SPECIFIC file or scope is named (--file,
# --local, --worktree, --global, --system, --blob) and ON for an effective read.
# So every SCOPED read in the guard silently skipped an `include.path`
# indirection that git's own effective resolution followed — and a config file
# can therefore arm rerere with the string 'rerere' appearing nowhere in its own
# bytes, which is what makes "I grepped the configs and they're clean" worthless
# as evidence.
#
# MEASURED on git 2.43.0 against the pre-fix guard, both halves:
#   sweep  — a lane whose config.worktree is nothing but `[include] path =
#            extra.cfg`, with `[rerere] enabled = true` in that sibling, has
#            EFFECTIVE rerere.enabled=true, yet `check <store>` exited 0 printing
#            NOTHING (`--file --get-regexp` returned exit 1, no output; the same
#            read with --includes returns `rerere.enabled true`).
#   shared — `.git/config` reaching rerere.enabled=true through its own
#            include.path, masked by the target lane's config.worktree, made
#            `check <lane>` exit 0 (`--local --get` exit 1, `--local --includes
#            --get` -> true).
echo ""
echo "--- (g-h) check follows include.path in its scoped reads ---"

read -r GH_REPO GH_A GH_B <<< "$(make_wt_repo)"
GH_A_GITDIR="$(git -C "$GH_A" rev-parse --absolute-git-dir)"

# (g-h-a) SWEEP path.  NEGATIVE CONTROL FIRST, before anything is planted, so no
# assertion below can pass merely because the new read fires unconditionally.
assert "(g-h-a) negative control: no include plant -> check exits 0" \
    bash "$GUARD" check "$GH_REPO"

printf '[include]\n\tpath = extra.cfg\n' > "$GH_A_GITDIR/config.worktree"
printf '[rerere]\n\tenabled = true\n' > "$GH_A_GITDIR/extra.cfg"

# LIVENESS, measured not assumed: git really does honour the indirection, so wtA
# IS armed.  Without this a later PASS could be the detector never firing.
assert "(g-h-a) fixture: git honours the include — wtA's EFFECTIVE rerere.enabled is true" \
    bash -c "[ \"\$(git -C '$GH_A' config --bool --get rerere.enabled)\" = true ]"

assert "(g-h-a) fixture: the string 'rerere' appears nowhere in that config.worktree" \
    bash -c "! grep -qi rerere '$GH_A_GITDIR/config.worktree'"

assert "(g-h-a) include-mediated lane override -> check exits non-zero" \
    bash -c "! bash '$GUARD' check '$GH_REPO' >/dev/null 2>&1"

assert "(g-h-a) ...and names the offending worktree" \
    bash -c "bash '$GUARD' check '$GH_REPO' 2>&1 >/dev/null | grep -q \"ARMED: worktree '.*wtA'\""

assert "(g-h-a) ...and does not name the innocent one" \
    bash -c "! bash '$GUARD' check '$GH_REPO' 2>&1 >/dev/null | grep -q 'wtB'"

# (g-h-b) SHARED-DEFAULT path — the same blind spot in `_check_shared_default`'s
# --local reads.  The lane masks the shared scope for ITSELF (the (g-f) shape),
# so only the shared-scope read can possibly see what the include contributes.
read -r GHB_REPO GHB_A GHB_B <<< "$(make_wt_repo)"
GHB_COMMON="$(common_dir "$GHB_REPO")"

git -C "$GHB_A" config --worktree rerere.enabled false
git -C "$GHB_A" config --worktree rerere.autoupdate false

assert "(g-h-b) negative control: shared false + lane self-disarm -> check exits 0" \
    bash "$GUARD" check "$GHB_A"

# Reach true through an include INSTEAD of a direct key: the direct keys are
# unset, so the shared read has nothing to find without following the chain.
git -C "$GHB_REPO" config --local --unset rerere.enabled
git -C "$GHB_REPO" config --local --unset rerere.autoupdate
printf '[rerere]\n\tenabled = true\n\tautoupdate = true\n' > "$GHB_COMMON/shared-extra.cfg"
git -C "$GHB_REPO" config --local include.path shared-extra.cfg

# Preconditions MEASURED: main armed, lane masked, and NO rr-cache/ — so the -1
# default branch cannot reach the right verdict for the wrong reason.
assert "(g-h-b) fixture: the MAIN checkout's EFFECTIVE rerere.enabled is true" \
    bash -c "[ \"\$(git -C '$GHB_REPO' config --bool --get rerere.enabled)\" = true ]"

assert "(g-h-b) fixture: ...while the LANE's effective value is false (the mask)" \
    bash -c "[ \"\$(git -C '$GHB_A' config --bool --get rerere.enabled)\" = false ]"

assert "(g-h-b) fixture: no rr-cache/ exists, so the -1 default cannot fire" \
    bash -c "! test -d '$GHB_COMMON/rr-cache'"

assert "(g-h-b) include-mediated SHARED arming -> check from the lane exits non-zero" \
    bash -c "! bash '$GUARD' check '$GHB_A' >/dev/null 2>&1"

assert "(g-h-b) ...naming the SHARED config as the armed scope" \
    bash -c "bash '$GUARD' check '$GHB_A' 2>&1 >/dev/null | grep -qF 'SHARED config sets rerere.enabled=true'"

# (g-h-c) rerere.autoupdate reached through the same chain is reported
# INDEPENDENTLY, not folded into the rerere.enabled verdict.
assert "(g-h-c) ...and reports rerere.autoupdate independently of rerere.enabled" \
    bash -c "bash '$GUARD' check '$GHB_A' 2>&1 >/dev/null | grep -qF 'SHARED config sets rerere.autoupdate=true'"

# The DIAGNOSTIC, not merely the exit code.  With a residual rr-cache present the
# pre-fix guard did report ARMED here — but through the wrong branch ("leaves
# rerere.enabled UNSET"), because its --local read could not see the include.  An
# exit-code-only assert would pass vacuously against that still-broken read.
mkdir -p "$GHB_COMMON/rr-cache/dddd4444"

assert "(g-h-b) with a residual rr-cache the verdict is still non-zero" \
    bash -c "! bash '$GUARD' check '$GHB_A' >/dev/null 2>&1"

assert "(g-h-b) ...and is NOT misattributed to the UNSET + rr-cache branch" \
    bash -c "! bash '$GUARD' check '$GHB_A' 2>&1 >/dev/null | grep -qF 'leaves rerere.enabled UNSET'"

assert "(g-h-b) ...it names the value the include actually contributes" \
    bash -c "bash '$GUARD' check '$GHB_A' 2>&1 >/dev/null | grep -qF 'SHARED config sets rerere.enabled=true'"

# (g-h-c) SWEEP side of the same independence claim: a lane whose include sets
# ONLY autoupdate must be reported for autoupdate and NOT for enabled.
read -r GHC_REPO GHC_A GHC_B <<< "$(make_wt_repo)"
GHC_A_GITDIR="$(git -C "$GHC_A" rev-parse --absolute-git-dir)"

assert "(g-h-c) negative control: no include plant -> check exits 0" \
    bash "$GUARD" check "$GHC_REPO"

printf '[include]\n\tpath = extra.cfg\n' > "$GHC_A_GITDIR/config.worktree"
printf '[rerere]\n\tautoupdate = true\n' > "$GHC_A_GITDIR/extra.cfg"

assert "(g-h-c) fixture: the lane's EFFECTIVE rerere.autoupdate is true" \
    bash -c "[ \"\$(git -C '$GHC_A' config --bool --get rerere.autoupdate)\" = true ]"

assert "(g-h-c) fixture: ...while its effective rerere.enabled stays false" \
    bash -c "[ \"\$(git -C '$GHC_A' config --bool --get rerere.enabled)\" = false ]"

assert "(g-h-c) an include-mediated autoupdate-only override -> check exits non-zero" \
    bash -c "! bash '$GUARD' check '$GHC_REPO' >/dev/null 2>&1"

assert "(g-h-c) ...naming rerere.autoupdate for that worktree" \
    bash -c "bash '$GUARD' check '$GHC_REPO' 2>&1 >/dev/null | grep -qF 'overrides rerere.autoupdate=true'"

assert "(g-h-c) ...and NOT claiming rerere.enabled is overridden there" \
    bash -c "! bash '$GUARD' check '$GHC_REPO' 2>&1 >/dev/null | grep -qF 'overrides rerere.enabled=true'"

# (g-h-d) `arm` SELF-HEALS the (g-h-b) store.  Its --local write lands in a
# [rerere] section APPENDED AFTER the [include] line, so it wins on git's
# last-wins precedence and the effective value really becomes false: exit 0, not
# the advisory 2.  The precondition is measured because it is the whole reason
# this case differs from (g-h-e): git 2.43.0 removes a section that `--unset`
# empties, so the shared config carries no [rerere] section for the write to be
# rewritten INTO.
assert "(g-h-d) fixture: the shared config has no [rerere] section for arm to rewrite" \
    bash -c "! grep -q '^\[rerere\]' '$GHB_COMMON/config'"

bash "$GUARD" arm "$GHB_A" >/dev/null 2>&1 && _ghd_status=0 || _ghd_status=$?

assert "(g-h-d) arm on an include-armed shared config exits 0 (self-healed)" \
    test "$_ghd_status" -eq 0

assert "(g-h-d) ...and the effective value really is false afterwards" \
    bash -c "[ \"\$(git -C '$GHB_REPO' config --bool --get rerere.enabled)\" = false ]"

assert "(g-h-d) ...via a direct pin appended after the [include] line" \
    bash -c "awk '/^\[include\]/{i=NR} /^\[rerere\]/{r=NR} END{exit !(i && r && i < r)}' '$GHB_COMMON/config'"

unset _ghd_status

# (g-h-e) `arm` stays HONEST when include ORDER defeats it.  With a [rerere]
# section already present BEFORE the [include], git rewrites that section in
# place, the later include still wins, and the effective value stays true.  The
# contract is exit 2 (advisory) — never a false 0 — with the surviving scope
# named.  MEASURED against the pre-fix guard: the write succeeds and the exit IS
# 2, but the WARNING enumerates only "another lane's config.worktree, or the
# user's global gitconfig" — a cause list that omits the actual cause.
read -r GHE_REPO GHE_A GHE_B <<< "$(make_wt_repo)"
GHE_COMMON="$(common_dir "$GHE_REPO")"
GHE_ERR="$_SUITE_TMP/arm-include-order.err"

git -C "$GHE_REPO" config --local rerere.enabled true
git -C "$GHE_REPO" config --local rerere.autoupdate true
printf '[rerere]\n\tenabled = true\n\tautoupdate = true\n' > "$GHE_COMMON/shared-extra.cfg"
git -C "$GHE_REPO" config --local include.path shared-extra.cfg

assert "(g-h-e) fixture: the [rerere] section precedes the [include] line" \
    bash -c "awk '/^\[rerere\]/{r=NR} /^\[include\]/{i=NR} END{exit !(r && i && r < i)}' '$GHE_COMMON/config'"

bash "$GUARD" arm "$GHE_REPO" >/dev/null 2>"$GHE_ERR" && _ghe_status=0 || _ghe_status=$?

assert "(g-h-e) fixture: the shared write DID land in the pre-existing section" \
    bash -c "[ \"\$(git -C '$GHE_REPO' config --local --get-all rerere.enabled | head -1)\" = false ]"

assert "(g-h-e) fixture: ...yet the later include still wins — effective stays true" \
    bash -c "[ \"\$(git -C '$GHE_REPO' config --bool --get rerere.enabled)\" = true ]"

assert "(g-h-e) arm exits exactly 2, not a false 0, when an include defeats the write" \
    test "$_ghe_status" -eq 2

assert "(g-h-e) ...printing an ARMED line rather than claiming success" \
    grep -q '^ARMED:' "$GHE_ERR"

assert "(g-h-e) ...and the WARNING names an include.path chain as a surviving scope" \
    grep -qF 'include.path chain in the shared config' "$GHE_ERR"

unset _ghe_status

# ── (g-i) LAST-WINS: the sweep must report git's RESOLUTION, not any value ────
# `--get-regexp` emits EVERY value a file sets for a key, while git resolves a
# multi-valued key to the LAST one.  The sweep flagged ARMED on any emitted
# `true`, so a config.worktree whose last word is `false` was reported armed
# even though git reads it as false.  MEASURED on git 2.43.0, WITHOUT any
# include (this pre-dates the include work and is not caused by it): a file with
# `[rerere] enabled = true` then `[rerere] enabled = false` has effective
# rerere.enabled=false in that lane, yet `check` printed "ARMED: worktree 'wtA'
# overrides rerere.enabled=true".  Adding --includes WIDENS it, because an
# included file can now contribute an overridden `true` of its own.
#
# A sweep false positive is uniquely damaging: `arm` writes --local only and can
# never clear a per-worktree file, so the store would sit at the advisory exit 2
# permanently with nothing actually wrong, and setup-dev.sh would warn on every
# developer setup forever.  That is the same reasoning (g-d) applies to an inert
# config.worktree.
echo ""
echo "--- (g-i) the sweep reports git's resolved value, not any emitted value ---"

read -r GI_REPO GI_A GI_B <<< "$(make_wt_repo)"
GI_A_GITDIR="$(git -C "$GI_A" rev-parse --absolute-git-dir)"
GI_B_GITDIR="$(git -C "$GI_B" rev-parse --absolute-git-dir)"

assert "(g-i) negative control: nothing planted -> check exits 0" \
    bash "$GUARD" check "$GI_REPO"

# (g-i-a) TWO PLAIN VALUES in one file, true then false.
printf '[rerere]\n\tenabled = true\n[rerere]\n\tenabled = false\n' > "$GI_A_GITDIR/config.worktree"

# Preconditions MEASURED: the armed value really IS emitted (so the detector had
# something to trip on and a PASS cannot be the read returning nothing) ...
assert "(g-i-a) fixture: --get-regexp emits BOTH values for the key" \
    bash -c "[ \"\$(git config --file '$GI_A_GITDIR/config.worktree' --includes --bool --get-regexp '^rerere\.enabled\$' | wc -l)\" -eq 2 ]"

assert "(g-i-a) fixture: ...the FIRST of which is the armed one" \
    bash -c "[ \"\$(git config --file '$GI_A_GITDIR/config.worktree' --includes --bool --get-regexp '^rerere\.enabled\$' | head -1)\" = 'rerere.enabled true' ]"

# ... while git RESOLVES the key to false, which is the verdict that matters.
assert "(g-i-a) fixture: git resolves the key to false — the last value wins" \
    bash -c "[ \"\$(git -C '$GI_A' config --bool --get rerere.enabled)\" = false ]"

assert "(g-i-a) an overridden true is NOT reported ARMED -> check exits 0" \
    bash "$GUARD" check "$GI_REPO"

assert "(g-i-a) ...and the lane is not named" \
    bash -c "! bash '$GUARD' check '$GI_REPO' 2>&1 >/dev/null | grep -q 'wtA'"

# (g-i-b) INCLUDE then DIRECT OVERRIDE — the shape --includes newly makes
# reachable: the included file arms the key, the config.worktree then disarms it
# directly, and git resolves to false.
printf '[include]\n\tpath = extra.cfg\n[rerere]\n\tenabled = false\n' > "$GI_B_GITDIR/config.worktree"
printf '[rerere]\n\tenabled = true\n' > "$GI_B_GITDIR/extra.cfg"

assert "(g-i-b) fixture: the include's armed value is emitted first" \
    bash -c "[ \"\$(git config --file '$GI_B_GITDIR/config.worktree' --includes --bool --get-regexp '^rerere\.enabled\$' | head -1)\" = 'rerere.enabled true' ]"

assert "(g-i-b) fixture: git resolves the key to false" \
    bash -c "[ \"\$(git -C '$GI_B' config --bool --get rerere.enabled)\" = false ]"

assert "(g-i-b) an include armed then directly disarmed -> check exits 0" \
    bash "$GUARD" check "$GI_REPO"

assert "(g-i-b) ...and neither lane is named" \
    bash -c "! bash '$GUARD' check '$GI_REPO' 2>&1 >/dev/null | grep -qE 'wtA|wtB'"

# (g-i-d) The operator-visible consequence: with nothing actually wrong, `arm`
# must succeed rather than park the store on the advisory exit 2 forever.
bash "$GUARD" arm "$GI_REPO" >/dev/null 2>&1 && _gi_status=0 || _gi_status=$?

assert "(g-i-d) arm on a store with only overridden values exits 0, not 2" \
    test "$_gi_status" -eq 0

unset _gi_status

# (g-i-c) REVERSE ORDER IS STILL CAUGHT.  The control that stops (g-i-a)/(g-i-b)
# from being satisfied by blanket suppression: direct `false` first, an
# `[include]` arming it after, so git resolves to TRUE and the lane really is
# armed.
read -r GIC_REPO GIC_A GIC_B <<< "$(make_wt_repo)"
GIC_A_GITDIR="$(git -C "$GIC_A" rev-parse --absolute-git-dir)"

assert "(g-i-c) negative control: nothing planted -> check exits 0" \
    bash "$GUARD" check "$GIC_REPO"

printf '[rerere]\n\tenabled = false\n[include]\n\tpath = extra.cfg\n' > "$GIC_A_GITDIR/config.worktree"
printf '[rerere]\n\tenabled = true\n' > "$GIC_A_GITDIR/extra.cfg"

assert "(g-i-c) fixture: the DISARMED value is emitted first, the armed one last" \
    bash -c "[ \"\$(git config --file '$GIC_A_GITDIR/config.worktree' --includes --bool --get-regexp '^rerere\.enabled\$' | head -1)\" = 'rerere.enabled false' ]"

assert "(g-i-c) fixture: git resolves the key to true — the lane IS armed" \
    bash -c "[ \"\$(git -C '$GIC_A' config --bool --get rerere.enabled)\" = true ]"

assert "(g-i-c) a value armed LAST is still reported -> check exits non-zero" \
    bash -c "! bash '$GUARD' check '$GIC_REPO' >/dev/null 2>&1"

assert "(g-i-c) ...naming the offending worktree" \
    bash -c "bash '$GUARD' check '$GIC_REPO' 2>&1 >/dev/null | grep -q \"ARMED: worktree '.*wtA'\""

# ── (g-j) AN UNRESOLVABLE INCLUDE CHAIN MUST BE UNKNOWN, NEVER CLEAN ──────────
# Following an include means the swept read can now FAIL on a file the guard
# never used to open — and that read is wrapped in `2>/dev/null || true`, which
# converts a failure into empty output, i.e. into the verdict "no rerere keys
# here, clean".  That is the very silent-false-clean class this whole round is
# closing, so it must not be re-opened at a new spot in the same round.  The
# failure mode does not exist before --includes: without it git never touches the
# included file at all.
#
# MEASURED on git 2.43.0, `git config --file <cfg> --includes --get-regexp ...`:
#   missing include target   -> exit 0 (or 1 when nothing else matches), silently
#                               ignored, direct keys still returned  [BENIGN]
#   CIRCULAR include chain   -> exit 128, "exceeded maximum include depth (10)",
#                               NO stdout
#   UNREADABLE include target-> exit 128, "unable to access ...: Permission
#                               denied", NO stdout
echo ""
echo "--- (g-j) an unresolvable include chain is reported UNKNOWN, not clean ---"

read -r GJ_REPO GJ_A GJ_B <<< "$(make_wt_repo)"
GJ_A_GITDIR="$(git -C "$GJ_A" rev-parse --absolute-git-dir)"

assert "(g-j) negative control: nothing planted -> check exits 0" \
    bash "$GUARD" check "$GJ_REPO"

# A genuine cycle: config.worktree -> loop.cfg -> config.worktree -> ...
printf '[include]\n\tpath = loop.cfg\n' > "$GJ_A_GITDIR/config.worktree"
printf '[include]\n\tpath = config.worktree\n' > "$GJ_A_GITDIR/loop.cfg"

# (g-j-a) FIXTURE LIVENESS.  The scenario is only meaningful if git really does
# fail here — a version that quietly tolerated the cycle would make every
# assertion below pass for the wrong reason.
assert "(g-j-a) fixture: the swept read really FAILS on a circular chain" \
    bash -c "! git config --file '$GJ_A_GITDIR/config.worktree' --includes --bool --get-regexp '^rerere\.(enabled|autoupdate)\$' >/dev/null 2>&1"

assert "(g-j-a) fixture: ...emitting NOTHING on stdout, so it looks exactly like 'clean'" \
    bash -c "[ -z \"\$(git config --file '$GJ_A_GITDIR/config.worktree' --includes --bool --get-regexp '^rerere\.(enabled|autoupdate)\$' 2>/dev/null)\" ]"

# (g-j-b) The lane is reported UNKNOWN, in the same vocabulary the sweep already
# uses for a config.worktree it cannot read at all.
assert "(g-j-b) check WARNs, naming the config path" \
    bash -c "bash '$GUARD' check '$GJ_REPO' 2>&1 >/dev/null | grep -qF 'cannot read $GJ_A_GITDIR/config.worktree'"

assert "(g-j-b) ...and the worktree it belongs to" \
    bash -c "bash '$GUARD' check '$GJ_REPO' 2>&1 >/dev/null | grep -q \"skipping worktree '.*wtA'\""

assert "(g-j-b) ...reporting that lane as UNKNOWN rather than verified safe" \
    bash -c "bash '$GUARD' check '$GJ_REPO' 2>&1 >/dev/null | grep -qF 'UNKNOWN, not verified safe'"

# UNKNOWN is NOT armed: `arm` writes --local and cannot fix a lane the guard
# merely fails to read, so folding this into the armed verdict would strand the
# store on a permanent failure — the trap the extensions.worktreeConfig gate
# already avoids.
#
# But UNKNOWN is NOT "safe" either, and the exit code is `check`'s ONLY
# machine-readable channel: returning 0 here made it FAIL-OPEN for exactly the
# use the runbook proposes for it (a periodic probe for the unidentified
# re-armer), so a store whose one armed lane is a lane the guard cannot read
# would answer "the fleet is clean".  Hence a distinct third code, 3.
_gj_status=0
bash "$GUARD" check "$GJ_REPO" >/dev/null 2>&1 || _gj_status=$?

assert "(g-j-b) an unverifiable lane exits 3 (UNVERIFIABLE), NOT 0 (safe)" \
    test "$_gj_status" -eq 3

assert "(g-j-b) ...and NOT 1 either — UNKNOWN is still not ARMED" \
    test "$_gj_status" -ne 1

assert "(g-j-b) ...and the verdict counts the unverifiable worktrees" \
    bash -c "bash '$GUARD' check '$GJ_REPO' 2>&1 >/dev/null | grep -qF 'UNVERIFIABLE: 1 worktree(s)'"

assert "(g-j-b) ...and no ARMED line is printed for it" \
    bash -c "! bash '$GUARD' check '$GJ_REPO' 2>&1 >/dev/null | grep -q \"ARMED: worktree '.*wtA'\""

unset _gj_status

# (g-j-c) One broken lane must not abort the sweep and mask every LATER lane.
# The glob is lexical, so wtA (broken) is traversed before wtB (genuinely armed).
git -C "$GJ_B" config --worktree rerere.enabled true

assert "(g-j-c) the later ARMED worktree is still reported (the sweep continued)" \
    bash -c "bash '$GUARD' check '$GJ_REPO' 2>&1 >/dev/null | grep -q \"ARMED: worktree '.*wtB'\""

assert "(g-j-c) ...and the WARNING precedes it, so the skip really was traversed past" \
    bash -c "bash '$GUARD' check '$GJ_REPO' 2>&1 >/dev/null | awk '/cannot read/{w=NR} /ARMED: worktree/{a=NR} END{exit !(w && a && w < a)}'"

assert "(g-j-c) ...and the verdict is non-zero because of that armed lane" \
    bash -c "! bash '$GUARD' check '$GJ_REPO' >/dev/null 2>&1"

# ARMED WINS over UNVERIFIABLE.  This store is now BOTH (wtA unreadable, wtB
# armed), and it must report 1, not 3: the armed lane is the half an operator
# can act on, and `arm`'s re-verify distinguishes "an override survived my
# write" from "I could not read a lane" by exactly this code.
_gj_both=0
bash "$GUARD" check "$GJ_REPO" >/dev/null 2>&1 || _gj_both=$?

assert "(g-j-c) a store that is BOTH armed and unverifiable exits 1, not 3" \
    test "$_gj_both" -eq 1

unset _gj_both

# (g-j-d) NOISE FLOOR.  A missing include target is BENIGN — git ignores it — and
# an include contributing no rerere key is simply clean.  Neither may produce a
# warning, or the guard cries wolf on ordinary configs and operators learn to
# ignore it.
read -r GJD_REPO GJD_A GJD_B <<< "$(make_wt_repo)"
GJD_A_GITDIR="$(git -C "$GJD_A" rev-parse --absolute-git-dir)"
GJD_B_GITDIR="$(git -C "$GJD_B" rev-parse --absolute-git-dir)"

printf '[include]\n\tpath = absent.cfg\n' > "$GJD_A_GITDIR/config.worktree"
printf '[include]\n\tpath = irrelevant.cfg\n' > "$GJD_B_GITDIR/config.worktree"
printf '[core]\n\tbare = false\n' > "$GJD_B_GITDIR/irrelevant.cfg"

# Preconditions MEASURED: git tolerates both, exiting 1 (the ordinary "no
# matching key" answer) rather than 128.
assert "(g-j-d) fixture: a MISSING include target is tolerated — the read exits 1, not 128" \
    bash -c "git config --file '$GJD_A_GITDIR/config.worktree' --includes --bool --get-regexp '^rerere\.(enabled|autoupdate)\$' >/dev/null 2>&1; [ \$? -eq 1 ]"

assert "(g-j-d) fixture: an irrelevant include is likewise tolerated — exit 1" \
    bash -c "git config --file '$GJD_B_GITDIR/config.worktree' --includes --bool --get-regexp '^rerere\.(enabled|autoupdate)\$' >/dev/null 2>&1; [ \$? -eq 1 ]"

assert "(g-j-d) neither shape is armed -> check exits 0" \
    bash "$GUARD" check "$GJD_REPO"

assert "(g-j-d) ...and neither produces a WARNING" \
    bash -c "! bash '$GUARD' check '$GJD_REPO' 2>&1 >/dev/null | grep -q 'WARNING'"

# (g-j-e) UNREADABLE include target — the other exit-128 shape.  chmod 000 is
# meaningless as root, same as (g-g)/(h-g).  Permissions are restored INLINE, and
# the suite's cleanup chmods before `rm -rf` so an assert dying mid-block cannot
# leave a fixture the sweep in cleanup is unable to reclaim.
if [ "$(id -u)" -eq 0 ]; then
    echo "  SKIP: (g-j-e) running as root — chmod 000 does not deny root reads"
else
    read -r GJU_REPO GJU_A GJU_B <<< "$(make_wt_repo)"
    GJU_A_GITDIR="$(git -C "$GJU_A" rev-parse --absolute-git-dir)"

    assert "(g-j-e) negative control: nothing planted -> check exits 0" \
        bash "$GUARD" check "$GJU_REPO"

    printf '[include]\n\tpath = secret.cfg\n' > "$GJU_A_GITDIR/config.worktree"
    printf '[rerere]\n\tenabled = true\n' > "$GJU_A_GITDIR/secret.cfg"
    chmod 000 "$GJU_A_GITDIR/secret.cfg"

    assert "(g-j-e) fixture: the config.worktree ITSELF is readable — only the target is not" \
        bash -c "head -c1 '$GJU_A_GITDIR/config.worktree' >/dev/null 2>&1 && ! head -c1 '$GJU_A_GITDIR/secret.cfg' >/dev/null 2>&1"

    assert "(g-j-e) fixture: the swept read really FAILS on an unreadable include target" \
        bash -c "! git config --file '$GJU_A_GITDIR/config.worktree' --includes --bool --get-regexp '^rerere\.(enabled|autoupdate)\$' >/dev/null 2>&1"

    assert "(g-j-e) check WARNs, naming the config path and the worktree" \
        bash -c "bash '$GUARD' check '$GJU_REPO' 2>&1 >/dev/null | grep -qF 'cannot read $GJU_A_GITDIR/config.worktree'"

    assert "(g-j-e) ...reporting that lane as UNKNOWN rather than verified safe" \
        bash -c "bash '$GUARD' check '$GJU_REPO' 2>&1 >/dev/null | grep -qF 'UNKNOWN, not verified safe'"

    _gju_status=0
    bash "$GUARD" check "$GJU_REPO" >/dev/null 2>&1 || _gju_status=$?

    assert "(g-j-e) ...and UNKNOWN is not ARMED, so check exits 3 rather than 1" \
        test "$_gju_status" -eq 3

    # `arm` must not turn an unverifiable lane into a hard failure: setup-dev.sh
    # runs it under `set -e` and branches `0 | 2 | *`, so anything but the
    # advisory 2 here would abort every later setup step over a lane whose
    # config the guard merely cannot read — and `arm` writes --local, so it
    # could never fix it anyway.  The shared write must still have landed.
    _gju_arm=0
    bash "$GUARD" arm "$GJU_REPO" >/dev/null 2>&1 || _gju_arm=$?

    assert "(g-j-e) arm on an unverifiable store exits the advisory 2, not a failure" \
        test "$_gju_arm" -eq 2

    assert "(g-j-e) ...and still pinned the shared config (its half of the job)" \
        bash -c "[ \"\$(git -C '$GJU_REPO' config --local --bool --get rerere.enabled)\" = false ]"

    assert "(g-j-e) ...and says the lanes are unverified, not that an override survived" \
        bash -c "bash '$GUARD' arm '$GJU_REPO' 2>&1 >/dev/null | grep -qF 'could not be verified'"

    assert "(g-j-e) ...and does NOT claim rerere is STILL armed" \
        bash -c "! bash '$GUARD' arm '$GJU_REPO' 2>&1 >/dev/null | grep -qF 'STILL armed'"

    chmod 644 "$GJU_A_GITDIR/secret.cfg"
    unset GJU_REPO GJU_A GJU_B GJU_A_GITDIR _gju_status _gju_arm
fi

# ── (g-k) A STALE (PRUNABLE) WORKTREE ENTRY IS INERT, NOT ARMED ───────────────
# The sweep walks <common>/worktrees/*/config.worktree straight off disk.  When a
# lane's WORKING TREE is deleted without `git worktree prune`, the admin dir —
# config.worktree included — survives, and git marks the entry `prunable gitdir
# file points to non-existent location`.  No git command can ever run in that
# worktree again, so git will never read that config.worktree again: reporting it
# ARMED is an inert-config false positive of exactly the class the
# extensions.worktreeConfig gate already guards against, and the same uniquely
# damaging shape — `arm` writes --local and can never clear a per-worktree file,
# so the store would park on the advisory exit 2 and setup-dev.sh would warn on
# every developer setup until someone pruned by hand.
#
# Reify's pool creates and destroys lanes continuously (warm-lane-gc.sh), so a
# prunable entry is a realistic transient rather than a pathology.
echo ""
echo "--- (g-k) a deleted-but-unpruned worktree's config.worktree is inert ---"

read -r GK_REPO GK_A GK_B <<< "$(make_wt_repo)"
GK_A_GITDIR="$(git -C "$GK_A" rev-parse --absolute-git-dir)"

assert "(g-k) negative control: nothing planted -> check exits 0" \
    bash "$GUARD" check "$GK_REPO"

git -C "$GK_A" config --worktree rerere.enabled true

# POSITIVE CONTROL, asserted while the worktree is still LIVE: the plant really
# is one the sweep detects, so a clean verdict after the delete can only be the
# liveness gate firing, never the detector failing to see the file.
assert "(g-k) positive control: while the lane is LIVE the plant IS reported" \
    bash -c "! bash '$GUARD' check '$GK_REPO' >/dev/null 2>&1"

rm -rf "$GK_A"

# Fixture preconditions MEASURED, not assumed.
assert "(g-k) fixture: git itself calls the entry prunable" \
    bash -c "git -C '$GK_REPO' worktree list --porcelain | grep -q '^prunable'"

assert "(g-k) fixture: the admin dir and its config.worktree SURVIVE the delete" \
    bash -c "test -f '$GK_A_GITDIR/config.worktree' && grep -q 'true' '$GK_A_GITDIR/config.worktree'"

assert "(g-k) fixture: the surviving entry's gitdir names a path that is gone" \
    bash -c "! test -e \"\$(cat '$GK_A_GITDIR/gitdir')\""

assert "(g-k) a stale entry's armed config.worktree is NOT reported -> check exits 0" \
    bash "$GUARD" check "$GK_REPO"

assert "(g-k) ...and the stale lane is not named at all" \
    bash -c "! bash '$GUARD' check '$GK_REPO' 2>&1 >/dev/null | grep -q \"worktree '.*wtA'\""

# A stale entry is verified IRRELEVANT, not unverifiable: counting it as UNKNOWN
# would pin a lane-churning pool at exit 3 forever.
assert "(g-k) ...and is NOT counted as UNVERIFIABLE either" \
    bash -c "! bash '$GUARD' check '$GK_REPO' 2>&1 >/dev/null | grep -q 'UNVERIFIABLE'"

# The gate must not swallow the LIVE lanes with it.
git -C "$GK_B" config --worktree rerere.enabled true

assert "(g-k) a LIVE armed lane in the same store is still reported" \
    bash -c "bash '$GUARD' check '$GK_REPO' 2>&1 >/dev/null | grep -q \"ARMED: worktree '.*wtB'\""

assert "(g-k) ...so the verdict is non-zero for the live lane alone" \
    bash -c "! bash '$GUARD' check '$GK_REPO' >/dev/null 2>&1"

# The gate FAILS SAFE on a gitdir shape git does not write. Skipping is its only
# direction that can hide an armed lane, so an unresolvable pointer must report,
# not silence: a spurious ARMED is advisory, a spurious skip is invisible.
read -r GKS_REPO GKS_A GKS_B <<< "$(make_wt_repo)"
GKS_A_GITDIR="$(git -C "$GKS_A" rev-parse --absolute-git-dir)"
git -C "$GKS_A" config --worktree rerere.enabled true

printf 'some/relative/path/.git\n' > "$GKS_A_GITDIR/gitdir"

assert "(g-k) a RELATIVE gitdir pointer is treated as live, not silently skipped" \
    bash -c "bash '$GUARD' check '$GKS_REPO' 2>&1 >/dev/null | grep -q \"ARMED: worktree '.*wtA'\""

# An empty gitdir file is git's other prunable shape ("gitdir file does not
# exist"), and unlike the above it is unambiguous — no working tree can be named
# by it at all — so it stays a silent skip.
: > "$GKS_A_GITDIR/gitdir"

assert "(g-k) an EMPTY gitdir file is a stale entry -> check exits 0" \
    bash "$GUARD" check "$GKS_REPO"

# The gate's OTHER fail-safe direction: an UNREADABLE gitdir is not a prunable
# entry.  It used to share the stale entry's `return 1`, so the caller skipped it
# SILENTLY and did not count it UNKNOWN — a live, armed lane simply vanished from
# the sweep and `check` answered 0 = "safe AND fully verified", fail-open on the
# very channel exit 3 exists to close (and the shape the runbook §8.2 periodic
# probe would consume).  It was also inconsistent with the two sibling cases:
# (g-g) an unreadable config.worktree and (g-j-e) an unreadable include target
# are both correctly UNKNOWN.  chmod 000 is meaningless as root, same as those.
if [ "$(id -u)" -eq 0 ]; then
    echo "  SKIP: (g-k) unreadable-gitdir arm — running as root, chmod 000 does not deny root reads"
else
    read -r GKU_REPO GKU_A GKU_B <<< "$(make_wt_repo)"
    GKU_A_GITDIR="$(git -C "$GKU_A" rev-parse --absolute-git-dir)"

    assert "(g-k) negative control: nothing planted -> check exits 0" \
        bash "$GUARD" check "$GKU_REPO"

    git -C "$GKU_A" config --worktree rerere.enabled true

    # POSITIVE CONTROL while the gitdir is still readable: the plant really is
    # one the sweep detects, so a changed verdict afterwards can only be the
    # liveness gate, never the detector failing to see the file.
    assert "(g-k) positive control: with a readable gitdir the armed lane IS reported" \
        bash -c "bash '$GUARD' check '$GKU_REPO' 2>&1 >/dev/null | grep -q \"ARMED: worktree '.*wtA'\""

    chmod 000 "$GKU_A_GITDIR/gitdir"

    # Fixture preconditions MEASURED, not assumed: the file is still THERE (so
    # the entry is not prunable) and the lane is still genuinely armed.
    assert "(g-k) fixture: the gitdir file exists but is unreadable" \
        bash -c "test -e '$GKU_A_GITDIR/gitdir' && ! head -c1 '$GKU_A_GITDIR/gitdir' >/dev/null 2>&1"

    assert "(g-k) fixture: the lane git itself reads is STILL armed" \
        bash -c "[ \"\$(git -C '$GKU_A' config --get rerere.enabled)\" = true ]"

    _gku_status=0
    bash "$GUARD" check "$GKU_REPO" >/dev/null 2>&1 || _gku_status=$?

    assert "(g-k) an unreadable gitdir is UNVERIFIABLE, so check exits 3 — never 0" \
        test "$_gku_status" -eq 3

    assert "(g-k) ...WARNing on the gitdir path rather than skipping in silence" \
        bash -c "bash '$GUARD' check '$GKU_REPO' 2>&1 >/dev/null | grep -qF 'cannot read $GKU_A_GITDIR/gitdir'"

    assert "(g-k) ...naming the lane, so an operator knows which one to fix" \
        bash -c "bash '$GUARD' check '$GKU_REPO' 2>&1 >/dev/null | grep -q \"whether worktree '.*wtA' is live\""

    assert "(g-k) ...reporting UNKNOWN in the same vocabulary as the sibling cases" \
        bash -c "bash '$GUARD' check '$GKU_REPO' 2>&1 >/dev/null | grep -qF 'UNKNOWN, not verified safe'"

    # `arm` must not turn an unverifiable lane into a hard failure — setup-dev.sh
    # branches 0 | 2 | * under `set -e`, and `arm` writes --local so it could
    # never repair a lane it cannot even locate.
    _gku_arm=0
    bash "$GUARD" arm "$GKU_REPO" >/dev/null 2>&1 || _gku_arm=$?

    assert "(g-k) arm over an unreadable gitdir exits the advisory 2, not a failure" \
        test "$_gku_arm" -eq 2

    assert "(g-k) ...and still pinned the shared config (its half of the job)" \
        bash -c "[ \"\$(git -C '$GKU_REPO' config --local --bool --get rerere.enabled)\" = false ]"

    # The gate must not swallow the LIVE lanes with it.
    git -C "$GKU_B" config --worktree rerere.enabled true

    assert "(g-k) a readable armed lane in the same store is still reported" \
        bash -c "bash '$GUARD' check '$GKU_REPO' 2>&1 >/dev/null | grep -q \"ARMED: worktree '.*wtB'\""

    # ARMED wins over UNVERIFIABLE, exactly as the header promises.
    _gku_both=0
    bash "$GUARD" check "$GKU_REPO" >/dev/null 2>&1 || _gku_both=$?

    assert "(g-k) ...and a store that is BOTH armed and unverifiable exits 1, not 3" \
        test "$_gku_both" -eq 1

    chmod 644 "$GKU_A_GITDIR/gitdir"
    unset GKU_REPO GKU_A GKU_B GKU_A_GITDIR _gku_status _gku_arm _gku_both
fi

unset GK_REPO GK_A GK_B GK_A_GITDIR GKS_REPO GKS_A GKS_B GKS_A_GITDIR

# ── (g-l) AN INHERITED GLOBAL/SYSTEM VALUE IS WHAT AN UNPINNED LANE ACTUALLY GETS
# `_check_shared_default` compared only the --local scope against the effective
# value, so it treated "unset in .git/config" as "git falls back to its built-in
# default".  It does not: precedence is system < global < local < worktree, so an
# explicit value in ~/.gitconfig or /etc/gitconfig is what a lane with no local
# pin really inherits, and the -1 fallback is never reached.
#
# The consequence was a FALSE POSITIVE with a diagnostic naming the wrong file: a
# store disarmed by a global `rerere.enabled = false` was reported ARMED,
# "because **its own config** overrides the shared default" — when nothing in
# $TARGET's own config was involved and no lane of the store was armed at all.
# Entirely untested until now, because the suite exports GIT_CONFIG_GLOBAL=
# /dev/null for hermeticity: these are the only fixtures that point it at a REAL
# file, and they do so per-invocation so the rest of the suite stays hermetic.
echo ""
echo "--- (g-l) the shared default resolves through global/system, not just --local ---"

GL_REPO="$(make_repo)"
GL_COMMON="$(common_dir "$GL_REPO")"
GL_GLOBAL="$GL_REPO.gitconfig"

# The -1-default shape: keys unset in the shared config, rr-cache/ on disk.
mkdir -p "$GL_COMMON/rr-cache"
printf '[rerere]\n\tenabled = false\n\tautoupdate = false\n' > "$GL_GLOBAL"

# Fixture preconditions MEASURED, not assumed.
assert "(g-l) fixture: rerere.enabled is unset in the SHARED config" \
    bash -c "! git -C '$GL_REPO' config --local --get rerere.enabled >/dev/null 2>&1"

assert "(g-l) fixture: rr-cache/ is on disk, so git's -1 default would arm the store" \
    test -d "$GL_COMMON/rr-cache"

assert "(g-l) fixture: with the global in play the EFFECTIVE value really is false" \
    bash -c "[ \"\$(GIT_CONFIG_GLOBAL='$GL_GLOBAL' git -C '$GL_REPO' config --bool --get rerere.enabled)\" = false ]"

# POSITIVE CONTROL FIRST, with the suite's hermetic /dev/null global: this store
# genuinely IS armed by the -1 default, so a clean verdict below can only come
# from the inherited value, never from the detector going blind.
assert "(g-l) positive control: with NO global, the -1 default arms it -> check exits 1" \
    bash -c "! bash '$GUARD' check '$GL_REPO' >/dev/null 2>&1"

assert "(g-l) ...via the -1-default diagnostic, not the inherited one" \
    bash -c "bash '$GUARD' check '$GL_REPO' 2>&1 >/dev/null | grep -qF 'UNSET and $GL_COMMON/rr-cache exists'"

# The hazard itself.
assert "(g-l) an inherited global false disarms the store -> check exits 0, not 1" \
    bash -c "GIT_CONFIG_GLOBAL='$GL_GLOBAL' bash '$GUARD' check '$GL_REPO'"

assert "(g-l) ...and says nothing is ARMED, because nothing is" \
    bash -c "! GIT_CONFIG_GLOBAL='$GL_GLOBAL' bash '$GUARD' check '$GL_REPO' 2>&1 >/dev/null | grep -q 'ARMED'"

assert "(g-l) ...and does NOT blame \$TARGET's own config, which is not involved" \
    bash -c "! GIT_CONFIG_GLOBAL='$GL_GLOBAL' bash '$GUARD' check '$GL_REPO' 2>&1 >/dev/null | grep -qF 'its own config'"

# Safe but UNPINNED is still worth saying: the disarm lives outside the store, so
# it does not travel with the repo and one --global --unset re-arms every lane.
assert "(g-l) ...but NOTEs that the store is unpinned and recommends arm" \
    bash -c "GIT_CONFIG_GLOBAL='$GL_GLOBAL' bash '$GUARD' check '$GL_REPO' 2>&1 >/dev/null | grep -qF 'only by an inherited global/system gitconfig'"

# An inherited autoupdate=false is byte-for-byte git's own default, so saying
# anything about it would be pure noise on every developer's setup.
assert "(g-l) ...and stays quiet about rerere.autoupdate, whose default is already false" \
    bash -c "! GIT_CONFIG_GLOBAL='$GL_GLOBAL' bash '$GUARD' check '$GL_REPO' 2>&1 >/dev/null | grep -q 'leaves rerere.autoupdate unset'"

# LAST-WINS across a multi-valued global, resolved the way git resolves it.
printf '[rerere]\n\tenabled = true\n\tenabled = false\n\tautoupdate = false\n' > "$GL_GLOBAL"

assert "(g-l) a multi-valued global resolves LAST-WINS, so true-then-false is safe" \
    bash -c "GIT_CONFIG_GLOBAL='$GL_GLOBAL' bash '$GUARD' check '$GL_REPO'"

printf '[rerere]\n\tenabled = false\n\tautoupdate = false\n' > "$GL_GLOBAL"

# arm SELF-HEALS it: a --local write is exactly the pin the NOTE asks for.
assert "(g-l) arm pins it locally and exits 0" \
    bash -c "GIT_CONFIG_GLOBAL='$GL_GLOBAL' bash '$GUARD' arm '$GL_REPO'"

assert "(g-l) ...leaving the shared config explicitly false" \
    bash -c "[ \"\$(git -C '$GL_REPO' config --local --bool --get rerere.enabled)\" = false ]"

assert "(g-l) ...after which the NOTE is gone, since the store is pinned" \
    bash -c "! GIT_CONFIG_GLOBAL='$GL_GLOBAL' bash '$GUARD' check '$GL_REPO' 2>&1 >/dev/null | grep -qF 'only by an inherited global/system gitconfig'"

# ── (g-l-b) THE MIRROR CASE: an inherited TRUE, masked by the target's own
# config.worktree.  The effective read at $TARGET comes back false, so cmd_check
# hands off to _check_shared_default — which, reading --local only, saw "unset"
# and returned clean while every OTHER lane of the store inherited the true.
GLT_REPO="$(make_repo)"
GLT_GLOBAL="$GLT_REPO.gitconfig"
git -C "$GLT_REPO" config extensions.worktreeConfig true
git -C "$GLT_REPO" config --worktree rerere.enabled false
printf '[rerere]\n\tenabled = true\n' > "$GLT_GLOBAL"

assert "(g-l-b) fixture: rerere.enabled is unset in the SHARED config" \
    bash -c "! git -C '$GLT_REPO' config --local --get rerere.enabled >/dev/null 2>&1"

assert "(g-l-b) fixture: the target's own config.worktree masks the global -> effective false" \
    bash -c "[ \"\$(GIT_CONFIG_GLOBAL='$GLT_GLOBAL' git -C '$GLT_REPO' config --bool --get rerere.enabled)\" = false ]"

assert "(g-l-b) an inherited true that only the target masks IS armed -> check exits 1" \
    bash -c "! GIT_CONFIG_GLOBAL='$GLT_GLOBAL' bash '$GUARD' check '$GLT_REPO' >/dev/null 2>&1"

assert "(g-l-b) ...naming the scope actually responsible" \
    bash -c "GIT_CONFIG_GLOBAL='$GLT_GLOBAL' bash '$GUARD' check '$GLT_REPO' 2>&1 >/dev/null | grep -qF 'inherited from the user'\\''s global/system gitconfig'"

assert "(g-l-b) arm SELF-HEALS it — a --local pin beats global" \
    bash -c "GIT_CONFIG_GLOBAL='$GLT_GLOBAL' bash '$GUARD' arm '$GLT_REPO'"

unset GL_REPO GL_COMMON GL_GLOBAL GLT_REPO GLT_GLOBAL

# ==============================================================================
# (h) `arm` — idempotently disable rerere in the SHARED local config, preserving
#     rr-cache.  The whole point is that every lane inherits one shared
#     default with zero per-lane wiring, so the write must be --local and must
#     be visible from a freshly added linked worktree.
# ==============================================================================
echo ""
echo "--- (h) arm disables rerere in shared config ---"

REPO_ARM="$(make_repo)"
git -C "$REPO_ARM" config rerere.enabled true
git -C "$REPO_ARM" config rerere.autoupdate true

# Populate rr-cache with sentinel entries so (h-c) can prove they survive.
ARM_RR="$(common_dir "$REPO_ARM")/rr-cache"
mkdir -p "$ARM_RR/aaaa1111" "$ARM_RR/bbbb2222"
printf 'preimage\n' > "$ARM_RR/aaaa1111/preimage"

assert "(h-a) arm exits 0" \
    bash "$GUARD" arm "$REPO_ARM"

assert "(h-a) rerere.enabled is false in the LOCAL (shared) config afterwards" \
    bash -c "[ \"\$(git -C '$REPO_ARM' config --local --bool --get rerere.enabled)\" = false ]"

assert "(h-a) rerere.autoupdate is false in the LOCAL (shared) config afterwards" \
    bash -c "[ \"\$(git -C '$REPO_ARM' config --local --bool --get rerere.autoupdate)\" = false ]"

# The values must be SHARED, not per-worktree: a --worktree write would leave
# every other lane of the store armed while this one reads clean.  A freshly
# added worktree inheriting them is the direct evidence.
ARM_WT="$REPO_ARM-armwt"; _TMPDIRS+=("$ARM_WT")
git -C "$REPO_ARM" worktree add -q -b armwt "$ARM_WT" >/dev/null 2>&1

assert "(h-a) a FRESHLY added linked worktree inherits rerere.enabled=false" \
    bash -c "[ \"\$(git -C '$ARM_WT' config --bool --get rerere.enabled)\" = false ]"

assert "(h-a) ...and inherits rerere.autoupdate=false (write was --local, not --worktree)" \
    bash -c "[ \"\$(git -C '$ARM_WT' config --bool --get rerere.autoupdate)\" = false ]"

# (h-b) idempotence — a second run must be a byte-level no-op, since setup-dev.sh
# runs this on every invocation.
_arm_cd="$(common_dir "$REPO_ARM")"
_arm_snap="$(mktemp -d "$_SUITE_TMP/snap.XXXXXX")"
cp "$_arm_cd/config" "$_arm_snap/before2"

assert "(h-b) a second arm run also exits 0" \
    bash "$GUARD" arm "$REPO_ARM"

cp "$_arm_cd/config" "$_arm_snap/after2"

assert "(h-b) the second arm run leaves .git/config byte-identical" \
    cmp -s "$_arm_snap/before2" "$_arm_snap/after2"

# (h-c) arm PRESERVES rr-cache.  Deleting an rr-cache entry another live worktree
# still holds is exactly the operation that reproduces the segfault + stale-lock
# signature — the cure would re-cause the disease across the fleet.  step-5(f-c)
# already proved the explicit false neutralises the residual cache in place.
assert "(h-c) rr-cache/ still exists after arm" \
    test -d "$ARM_RR"

assert "(h-c) rr-cache entries are unchanged after arm" \
    bash -c "[ \"\$(ls -A '$ARM_RR' | sort | tr '\n' ' ')\" = 'aaaa1111 bbbb2222 ' ]"

assert "(h-c) an rr-cache entry's payload survives arm" \
    test -f "$ARM_RR/aaaa1111/preimage"

# (h-d) arm's own verdict agrees with check.
assert "(h-d) check exits 0 after arm" \
    bash "$GUARD" check "$REPO_ARM"

# ── (h-e) BEHAVIOURAL ORACLE, inside a LINKED WORKTREE ─────────────────────────
# The live hazard is a lane, not the main checkout: measure that a conflicted
# merge in a linked worktree of the armed repo records zero rr-cache entries and
# leaves no MERGE_RR behind.
echo ""
echo "--- (h-e) behavioural oracle: armed repo records nothing from a lane ---"

_arm_side="$(make_conflict "$ARM_WT")"
_arm_rr_before="$(count_rr_entries "$ARM_WT")"
git -C "$ARM_WT" merge "$_arm_side" >/dev/null 2>&1 || true
_arm_rr_after="$(count_rr_entries "$ARM_WT")"

assert "(h-e) the lane's merge really did conflict (oracle is live)" \
    bash -c "git -C '$ARM_WT' ls-files -u | grep -q ."

assert "(h-e) armed repo: a lane's conflicted merge records ZERO new rr-cache entries" \
    test "$_arm_rr_before" -eq "$_arm_rr_after"

_arm_wt_gitdir="$(git -C "$ARM_WT" rev-parse --absolute-git-dir)"

assert "(h-e) the lane's MERGE_RR is absent or empty" \
    bash -c "! test -s '$_arm_wt_gitdir/MERGE_RR'"

assert "(h-e) no MERGE_RR.lock was left behind in the lane" \
    bash -c "! test -e '$_arm_wt_gitdir/MERGE_RR.lock'"

git -C "$ARM_WT" merge --abort >/dev/null 2>&1 || true
unset _arm_cd _arm_snap _arm_rr_before _arm_rr_after _arm_wt_gitdir _arm_side

# ── (h-f) arm's FAILURE branch: a foreign per-worktree override survives ──────
# `arm` writes --local only, so it can never clear ANOTHER lane's config.worktree
# — the dominant way its post-write re-verify still reports armed. That case must
# be distinguishable by exit code from "the shared write itself failed", because
# setup-dev.sh runs `arm` under `set -e` on every developer setup: one self-armed
# self-armed lane must not abort everything after it. Contract:
#   0 = disarmed and verified
#   2 = shared config pinned, but an override this run cannot reach survives
#   any other non-zero = a failure of this run (1, or git's own status if a
#     config write aborts under `set -e`) — see (h-g)
# This branch had no test at all, despite deciding whether setup-dev.sh aborts.
echo ""
echo "--- (h-f) arm reports a surviving foreign override as exit 2 ---"

read -r AF_REPO AF_A AF_B <<< "$(make_wt_repo)"
git -C "$AF_REPO" config rerere.enabled true
git -C "$AF_REPO" config rerere.autoupdate true
git -C "$AF_A" config --worktree rerere.enabled true

bash "$GUARD" arm "$AF_REPO" >/dev/null 2>&1 && _af_status=0 || _af_status=$?

assert "(h-f) arm exits 2 (not 0, not 1) when a foreign override survives" \
    test "$_af_status" -eq 2

# arm must still have done its half of the job: the fleet-wide default is pinned
# even though this one lane overrides it for itself.
assert "(h-f) arm still wrote rerere.enabled=false to the shared config" \
    bash -c "[ \"\$(git -C '$AF_REPO' config --local --bool --get rerere.enabled)\" = false ]"

assert "(h-f) arm still wrote rerere.autoupdate=false to the shared config" \
    bash -c "[ \"\$(git -C '$AF_REPO' config --local --bool --get rerere.autoupdate)\" = false ]"

assert "(h-f) arm names the offending worktree so an operator can act" \
    bash -c "bash '$GUARD' arm '$AF_REPO' 2>&1 >/dev/null | grep -q 'wtA'"

unset _af_status

# ── (h-g) A FAILED SHARED WRITE must produce the documented exit 1 ────────────
# The contract says 0 = disarmed, 2 = advisory, and a failure of THIS RUN is
# distinguishable from both. It was not producible: the write was a bare
# `git -C "$TARGET" config --local "$key" false` under `set -euo pipefail`, so a
# real failure — a lost race on .git/config.lock (every lane of the store writes
# that ONE file), a read-only store, a multi-valued key — aborted the script with
# git's own status, never 1. A consumer branching `1 => fatal; 2 => warn;
# else => ok` would have read a failed write as SUCCESS, and the fleet would stay
# armed with nothing in the log to say so.
echo ""
echo "--- (h-g) a failed shared-config write exits 1, not git's own status ---"

if [ "$(id -u)" -eq 0 ]; then
    echo "  SKIP: (h-g) running as root — chmod cannot make the store unwritable"
else
    RO_REPO="$(make_repo)"
    git -C "$RO_REPO" config rerere.enabled true
    RO_COMMON="$(common_dir "$RO_REPO")"
    RO_ERR="$_SUITE_TMP/arm-readonly.err"

    # Write permission is dropped on the git DIR, not on config itself: git writes
    # config through a config.lock sibling and renames, so a read-only file alone
    # would not reproduce the failure.
    chmod a-w "$RO_COMMON"

    # Precondition MEASURED, not assumed — if the store were somehow still
    # writable, every assertion below would be vacuous.
    _ro_precond=0
    git -C "$RO_REPO" config --local rerere.autoupdate false >/dev/null 2>&1 || _ro_precond=$?

    bash "$GUARD" arm "$RO_REPO" >/dev/null 2>"$RO_ERR" && _ro_status=0 || _ro_status=$?

    chmod u+w "$RO_COMMON"

    assert "(h-g) fixture: the store really is unwritable (a git config write failed)" \
        test "$_ro_precond" -ne 0

    assert "(h-g) arm on an unwritable store exits exactly 1, not git's own status" \
        test "$_ro_status" -eq 1

    assert "(h-g) ...and says the shared config was NOT pinned" \
        grep -qF "shared config was NOT pinned" "$RO_ERR"

    assert "(h-g) ...naming the config path an operator has to look at" \
        grep -qF "$RO_COMMON/config" "$RO_ERR"

    # The failure must be reported as a failure, not dressed up as the SET line
    # a successful write emits.
    assert "(h-g) ...and does NOT claim it set the key" \
        bash -c "! grep -qF 'SET: rerere.enabled=false' '$RO_ERR'"

    unset RO_REPO RO_COMMON RO_ERR _ro_status _ro_precond
fi

# ── (h-h) A MULTI-VALUED SHARED KEY must SELF-HEAL, not hard-abort setup ──────
# `git config --add rerere.enabled true` — precisely what an unidentified
# re-armer would leave behind (runbook §7) — makes the key multi-valued, and a
# plain single-value write then FAILS: measured on git 2.43.0, `git config
# --local rerere.enabled false` on a `true`/`true` shared config reports
# `error: cannot overwrite multiple values with a single value` and exits 5.
# That was not inert: the guarded branch returned 1, and setup-dev.sh's `*` arm
# turns that into `err` + `exit 1`, killing the build-accelerator systemd block,
# npm and the smoke test for every developer — while the fleet stayed ARMED,
# which is the exact failure this guard exists to prevent.
#
# --replace-all is a strict superset of the old write: byte-identical for the
# unset and single-valued cases (pinned below), and it collapses a multi-valued
# key to one `false` instead of failing.
echo ""
echo "--- (h-h) arm self-heals a multi-valued shared key ---"

MV_REPO="$(make_repo)"
git -C "$MV_REPO" config --add rerere.enabled true
git -C "$MV_REPO" config --add rerere.enabled true
git -C "$MV_REPO" config --add rerere.autoupdate true
git -C "$MV_REPO" config --add rerere.autoupdate true

# FIXTURE LIVENESS.  Assert the old write really does fail here, so a later PASS
# cannot be --replace-all quietly papering over a shape git never minded.
_mv_precond=0
git -C "$MV_REPO" config --local rerere.enabled false >/dev/null 2>&1 || _mv_precond=$?

assert "(h-h) fixture: a single-value write really FAILS on the multi-valued key" \
    test "$_mv_precond" -ne 0

assert "(h-h) fixture: ...and the key really is multi-valued (two values, both true)" \
    bash -c "[ \"\$(git -C '$MV_REPO' config --local --get-all rerere.enabled | tr '\n' ' ')\" = 'true true ' ]"

_mv_status=0
bash "$GUARD" arm "$MV_REPO" >/dev/null 2>&1 || _mv_status=$?

assert "(h-h) arm exits 0 on a multi-valued shared key (self-healed, not aborted)" \
    test "$_mv_status" -eq 0

assert "(h-h) rerere.enabled collapses to a SINGLE resolved false" \
    bash -c "[ \"\$(git -C '$MV_REPO' config --local --get-all rerere.enabled | tr '\n' ' ')\" = 'false ' ]"

assert "(h-h) rerere.autoupdate collapses to a SINGLE resolved false" \
    bash -c "[ \"\$(git -C '$MV_REPO' config --local --get-all rerere.autoupdate | tr '\n' ' ')\" = 'false ' ]"

assert "(h-h) ...so the effective value a lane inherits is false" \
    bash -c "[ \"\$(git -C '$MV_REPO' config --bool --get rerere.enabled)\" = false ]"

assert "(h-h) check agrees afterwards" \
    bash "$GUARD" check "$MV_REPO"

# A `true` followed by a `false` resolves to false ALREADY, so the old
# --get probe skipped the write and left the stale `true` line in place — one
# `git config --unset` away from re-arming the fleet.  --get-all sees the
# literal set of values, so the skip fires only on a single, genuine `false`.
MV2_REPO="$(make_repo)"
git -C "$MV2_REPO" config --add rerere.enabled true
git -C "$MV2_REPO" config --add rerere.enabled false

assert "(h-h) fixture: git already resolves true-then-false to false" \
    bash -c "[ \"\$(git -C '$MV2_REPO' config --bool --get rerere.enabled)\" = false ]"

assert "(h-h) arm on an already-false-by-last-wins key exits 0" \
    bash "$GUARD" arm "$MV2_REPO"

assert "(h-h) ...and REMOVES the stale 'true' line rather than leaving it latent" \
    bash -c "[ \"\$(git -C '$MV2_REPO' config --local --get-all rerere.enabled | tr '\n' ' ')\" = 'false ' ]"

# IDEMPOTENCE is unchanged by --replace-all: the ordinary single-valued re-run
# must still be a byte-level no-op, since setup-dev.sh runs `arm` every time.
_mv_cd="$(common_dir "$MV_REPO")"
cp "$_mv_cd/config" "$_SUITE_TMP/mv.before"

assert "(h-h) a re-run on the healed store exits 0" \
    bash "$GUARD" arm "$MV_REPO"

assert "(h-h) ...and leaves .git/config byte-identical (--replace-all is still a no-op)" \
    cmp -s "$_SUITE_TMP/mv.before" "$_mv_cd/config"

unset MV_REPO MV2_REPO _mv_precond _mv_status _mv_cd

# ==============================================================================
# (i) `scan-locks` — the M3 recurrence detector.  A failed rr-cache preimage
#     write leaves a stale zero-byte MERGE_RR.lock in .git/worktrees/<lane>/,
#     after which every `git commit` in that lane exits 128 while the commit
#     object is still written and the ref still moves.
# ==============================================================================
echo ""
echo "--- (i) scan-locks censuses stale MERGE_RR.lock files ---"

read -r LK_REPO LK_A LK_B <<< "$(make_wt_repo)"
LK_COMMON="$(common_dir "$LK_REPO")"
LK_A_GITDIR="$(git -C "$LK_A" rev-parse --absolute-git-dir)"
LK_B_GITDIR="$(git -C "$LK_B" rev-parse --absolute-git-dir)"

# (i-b) clean store first, so a later PASS cannot be the detector never firing.
assert "(i-b) no MERGE_RR.lock anywhere -> scan-locks exits 0" \
    bash "$GUARD" scan-locks "$LK_REPO"

# ── (i-c) NOISE FLOOR ─────────────────────────────────────────────────────────
# A bare MERGE_RR with no .lock sibling is ORDINARY rerere state, present in a
# large fraction of the live store's worktrees.  Reporting it would make the detector
# read as "the corruption is fleet-wide" when nothing is wrong at all.
: > "$LK_B_GITDIR/MERGE_RR"

assert "(i-c) a bare MERGE_RR (no .lock) -> scan-locks still exits 0" \
    bash "$GUARD" scan-locks "$LK_REPO"

assert "(i-c) a bare MERGE_RR worktree is NOT named in the output" \
    bash -c "! bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -q \"\$(basename '$LK_B_GITDIR')\""

# (i-a) planted stale lock is reported, naming the worktree.
: > "$LK_A_GITDIR/MERGE_RR.lock"

assert "(i-a) planted MERGE_RR.lock -> scan-locks exits non-zero" \
    bash -c "! bash '$GUARD' scan-locks '$LK_REPO' >/dev/null 2>&1"

assert "(i-a) scan-locks names the offending worktree" \
    bash -c "bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -q \"\$(basename '$LK_A_GITDIR')\""

assert "(i-a) scan-locks classifies it STALE" \
    bash -c "bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -qi 'stale'"

assert "(i-a) scan-locks prints the exact rm remediation command" \
    bash -c "bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -q 'rm -f $LK_A_GITDIR/MERGE_RR.lock'"

# ── (i-d) READ-ONLY ───────────────────────────────────────────────────────────
# Deletion stays manual and operator-driven: this script must never mutate
# another lane's git dir, so the planted files must survive the census.
assert "(i-d) scan-locks did NOT delete the MERGE_RR.lock" \
    test -e "$LK_A_GITDIR/MERGE_RR.lock"

assert "(i-d) scan-locks did NOT delete the bare MERGE_RR" \
    test -e "$LK_B_GITDIR/MERGE_RR"

# ── (i-e) OPERATION IN PROGRESS ───────────────────────────────────────────────
# A lock alongside a real in-flight operation is NOT stale — telling an operator
# to rm it would destroy that operation's state. Classify, do not lump.
#
# PARAMETERISED over every marker _classify_lock checks. Previously only
# MERGE_HEAD was ever planted, so five of the six could have been typo'd or
# dropped and this suite would have stayed green while an operator was told to
# `rm -f` a lock guarding a live rebase, cherry-pick or revert.
#
# The old positive assertion was also VACUOUS: it grepped for
# 'in-progress\|in progress', and the STALE message itself ends "...with no
# operation in progress." — so LK_A's stale lock satisfied it no matter what
# _classify_lock did with LK_B. The assertions below match the full
# "OPERATION-IN-PROGRESS: <label> ... alongside <marker>." line instead, which
# pins each marker individually.
_LKB_LABEL="$(basename "$LK_B_GITDIR")"

# NEGATIVE CONTROL FIRST: the same lock with NO marker is STALE and IS offered an
# rm, so the per-marker negatives below cannot pass by the rm never being offered
# for this lane at all.
: > "$LK_B_GITDIR/MERGE_RR.lock"

assert "(i-e) baseline: the same lock with no marker is STALE and offered an rm" \
    bash -c "bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -qF 'rm -f $LK_B_GITDIR/MERGE_RR.lock'"

for _marker in MERGE_HEAD MERGE_MSG CHERRY_PICK_HEAD REVERT_HEAD rebase-merge rebase-apply; do
    # Exactly one marker present per iteration, so _classify_lock's first-match
    # break is deterministic and the assertion names the marker it planted.
    rm -rf "$LK_B_GITDIR/MERGE_HEAD" "$LK_B_GITDIR/MERGE_MSG" \
           "$LK_B_GITDIR/CHERRY_PICK_HEAD" "$LK_B_GITDIR/REVERT_HEAD" \
           "$LK_B_GITDIR/rebase-merge" "$LK_B_GITDIR/rebase-apply"
    case "$_marker" in
        # git's rebase state is a DIRECTORY, not a file — `[ -e ]` covers both,
        # and planting the real shape is what keeps that true.
        rebase-merge|rebase-apply) mkdir -p "$LK_B_GITDIR/$_marker" ;;
        *)                         git -C "$LK_B" rev-parse HEAD > "$LK_B_GITDIR/$_marker" ;;
    esac

    assert "(i-e) lock + $_marker -> OPERATION-IN-PROGRESS naming $_marker" \
        bash -c "bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -qF \"OPERATION-IN-PROGRESS: $_LKB_LABEL holds MERGE_RR.lock alongside $_marker.\""

    assert "(i-e) ...and the $_marker lane is NOT offered an rm command" \
        bash -c "! bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -qF 'rm -f $LK_B_GITDIR/MERGE_RR.lock'"
done
unset _marker

# LK_A's stale lock is untouched throughout, so the STALE arm keeps working while
# a sibling lane is mid-operation — the two classifications are per-lane, not a
# whole-census verdict.
assert "(i-e) the sibling STALE lane is still offered its own rm command" \
    bash -c "bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -qF 'rm -f $LK_A_GITDIR/MERGE_RR.lock'"

assert "(i-e) scan-locks still exits non-zero when a lock exists at all" \
    bash -c "! bash '$GUARD' scan-locks '$LK_REPO' >/dev/null 2>&1"

# ── (i-f) THE MAIN CHECKOUT is a scan target too ──────────────────────────────
# For the main checkout, git dir == common dir, so its stale lock lands at
# <common>/MERGE_RR.lock and NEVER under worktrees/. That is not a hypothetical
# location: the sanctioned landing path (scripts/land.sh, CLAUDE.md "Landing on
# main") runs a real `git merge --no-ff` in the main checkout, making it an
# active merge site and therefore an active candidate for this very signature. A
# census blind to it reports "clean" while every `git commit` on main exits 128.
echo ""
echo "--- (i-f) scan-locks covers the main checkout's own git dir ---"

read -r MC_REPO MC_A MC_B <<< "$(make_wt_repo)"
MC_COMMON="$(common_dir "$MC_REPO")"

# Clean baseline first, so a later PASS cannot be the detector never firing.
assert "(i-f) baseline: a clean store exits 0" \
    bash "$GUARD" scan-locks "$MC_REPO"

: > "$MC_COMMON/MERGE_RR.lock"

assert "(i-f) a lock in the MAIN checkout's git dir -> scan-locks exits non-zero" \
    bash -c "! bash '$GUARD' scan-locks '$MC_REPO' >/dev/null 2>&1"

assert "(i-f) ...classified STALE with the exact rm remediation" \
    bash -c "bash '$GUARD' scan-locks '$MC_REPO' 2>&1 | grep -q 'rm -f $MC_COMMON/MERGE_RR.lock'"

assert "(i-f) ...and read-only: the lock survives the census" \
    test -e "$MC_COMMON/MERGE_RR.lock"

# The in-progress classification must apply to the main checkout as well —
# land.sh's merge is exactly when a live MERGE_HEAD sits beside the lock.
git -C "$MC_REPO" rev-parse HEAD > "$MC_COMMON/MERGE_HEAD"

assert "(i-f) main-checkout lock + MERGE_HEAD -> reported as operation-in-progress" \
    bash -c "bash '$GUARD' scan-locks '$MC_REPO' 2>&1 | grep -qF -- '<main checkout> holds MERGE_RR.lock alongside MERGE_HEAD.'"

assert "(i-f) ...and is NOT offered an rm command" \
    bash -c "! bash '$GUARD' scan-locks '$MC_REPO' 2>&1 | grep -q 'rm -f $MC_COMMON/MERGE_RR.lock'"

rm -f "$MC_COMMON/MERGE_HEAD" "$MC_COMMON/MERGE_RR.lock"

# A store with NO linked worktrees must still have its main dir scanned: the
# missing worktrees/ dir is a normal shape, not a reason to skip the census.
NOWT_REPO="$(make_repo)"
NOWT_COMMON="$(common_dir "$NOWT_REPO")"

assert "(i-f) fixture: the no-worktree store really has no worktrees/ dir" \
    bash -c "! test -d '$NOWT_COMMON/worktrees'"

: > "$NOWT_COMMON/MERGE_RR.lock"

assert "(i-f) a store with no linked worktrees still reports its main-dir lock" \
    bash -c "! bash '$GUARD' scan-locks '$NOWT_REPO' >/dev/null 2>&1"

# The noise floor applies to the main dir too: a bare MERGE_RR is ordinary state.
rm -f "$NOWT_COMMON/MERGE_RR.lock"
: > "$NOWT_COMMON/MERGE_RR"

assert "(i-f) a bare MERGE_RR in the main dir is NOT reported (noise floor)" \
    bash "$GUARD" scan-locks "$NOWT_REPO"

# ==============================================================================
# (wiring) / (wiring-lane) — the guard must actually be INVOKED, at BOTH cadences.
#
# These are BEHAVIOURAL pins, not documentation assertions: without them the
# guard is dead code, and the invariant it enforces would silently revert the
# first time anything rewrote the shared .git/config.  Mirrors the (wiring)
# block in tests/infra/test_main_gate_worktree_config.sh.
#
# THE DETECTOR ANCHORS ON THE EXECUTED COMMAND SHAPE, NOT A FILENAME MENTION.
# The first cut of these blocks grepped for the bare string `git-rerere-guard.sh`
# and for `git-rerere-guard\.sh["'[:space:]]+arm`.  Both were VACUOUS — measured,
# not theorised: with the ENTIRE seed-side invocation block commented out, both
# still exited 0, satisfied by non-comment PROSE elsewhere in the same files:
#   * scripts/seed-warm-lane.sh:330 — the _usage heredoc line
#     "REIFY_WARM_LANE_RERERE_ARM=0  Skip the shared-store `git-rerere-guard.sh
#     arm`", which the old arm-regex literally printed as its match;
#   * scripts/seed-warm-lane.sh:1677 and scripts/setup-dev.sh:370 — the failure
#     warnings, `warn "git-rerere-guard.sh arm failed (exit $_rerere_arm_rc)"`.
# None of those lines starts with `#`, so the comment prefilter never stripped
# them, and the asserts could not detect the exact regression their own comments
# claimed they existed to catch.
#
# $_RERERE_CALL_RE below instead demands a path-prefixed, variable-rooted, QUOTED
# invocation followed by the `arm` subcommand — a shape prose cannot accidentally
# take, since both decoys begin with `REIFY_…` / `warn ` rather than `"$`.
# MEASURED in both directions on each script: exactly one matching line, exit 1
# once that line is commented out, and exit 1 if `arm` is downgraded to `check`.
#
# WHY `arm` AND NOT `check`: a bare `check` would REPORT the drift while leaving
# all ~253 lanes armed.  Only `arm` makes the invariant self-healing.
#
# WHY LANE CADENCE IS THE POINT (the second block): setup-dev.sh runs at
# DEVELOPER-SETUP cadence — between two setup runs nothing re-pins the shared
# config, so the store can sit armed for as long as a developer goes without
# re-running setup.  The re-arm rate measured on the live store makes that window
# unacceptable: /home/leo/src/reify's shared .git/config was found ARMED TWICE in
# a single day, 2026-08-30 07:06:11 and 11:44:46, both 2508 bytes (the armed
# size; the guard's own disarm write leaves 2509, so the one-byte delta
# discriminates a third-party write from the guard's).  seed-warm-lane.sh
# --fresh-checkout runs on EVERY lane ACQUIRE, narrowing the exposure window from
# "since the last developer setup" to "since the last acquire".
# ==============================================================================

# Matches the INVOCATION and nothing else.  Kept behind the `grep -Ev
# '^[[:space:]]*#'` prefilter belt-and-braces: the `^[[:space:]]*"?\$` anchor
# already breaks under a `# ` prefix, but the prefilter costs nothing and keeps
# both blocks reading the same way.
_RERERE_CALL_RE='^[[:space:]]*"?\$.*/git-rerere-guard\.sh"[[:space:]]+arm([[:space:]]|$)'

# _guard_call_present <script> — the detector under test.  Exit 0 iff <script>
# carries an uncommented, correctly-shaped `"$…/git-rerere-guard.sh" arm …` call.
_guard_call_present() {
    local _n
    # `grep -Ec`, deliberately NOT `grep -Eq`: -q exits at the FIRST match and
    # closes the pipe, so the upstream `grep -Ev` dies of SIGPIPE (141) and
    # `set -o pipefail` turns a genuine MATCH into a non-zero return.  That is
    # not theoretical — the -q form was written first and produced a spurious
    # FAIL of the setup-dev assert during this block's own mutation testing,
    # racily, on a file where the assert had just passed.  -c reads all input.
    _n="$(grep -Ev '^[[:space:]]*#' "$1" | grep -Ec "$_RERERE_CALL_RE")" || true
    [ "${_n:-0}" -ge 1 ]
}

_guard_call_absent() {
    ! _guard_call_present "$1"
}

# _without_guard_call <script> — print the path of a throwaway copy of <script>
# with its guard invocation commented out, for use as a NEGATIVE CONTROL.
#
# The line is located BY MATCHING IT, never by line number: hard-coded numbers
# drift on every later edit to these 700/1700-line scripts and would silently
# turn the negative control into a no-op.
#
# `%` as the s/// delimiter, NOT `|`: $_RERERE_CALL_RE contains a literal `|`
# (the `([[:space:]]|$)` alternation), so a `s|…|…|` form ends the regex early
# and sed dies with "Unmatched ( or \(" — leaving an EMPTY output file, which
# "differs from the original" and makes the detector exit 1 for the wrong
# reason, i.e. a vacuously-passing negative control.  Hit for real while
# building this block; the caller-side liveness assert below is the backstop.
_without_guard_call() {
    local out
    out="$(mktemp "$_SUITE_TMP/nocall.XXXXXX")"
    sed -E "s%$_RERERE_CALL_RE%# &%" "$1" > "$out"
    echo "$out"
}

# _mutation_is_live <orig> <mutated> — the negative control's own control.
# The mutation only PREFIXES one line, so the copy must (a) still exist with the
# same line count — catching an empty/truncated sed failure — and (b) actually
# differ — catching a regex that matched nothing.
_mutation_is_live() {
    [ -s "$2" ] || return 1
    [ "$(wc -l < "$1")" -eq "$(wc -l < "$2")" ] || return 1
    ! cmp -s "$1" "$2"
}

echo ""
echo "--- (wiring) setup-dev.sh calls git-rerere-guard.sh ---"

SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

assert "(wiring) scripts/setup-dev.sh exists" \
    test -f "$SETUP_DEV"

assert "(wiring) setup-dev.sh invokes '\"\$…/git-rerere-guard.sh\" arm' (uncommented)" \
    _guard_call_present "$SETUP_DEV"

SETUP_DEV_NOCALL="$(_without_guard_call "$SETUP_DEV")"

assert "(wiring) negative-control fixture is live (one line commented, nothing else)" \
    _mutation_is_live "$SETUP_DEV" "$SETUP_DEV_NOCALL"

assert "(wiring) detector FAILS when setup-dev.sh's guard call is commented out" \
    _guard_call_absent "$SETUP_DEV_NOCALL"

echo ""
echo "--- (wiring-lane) seed-warm-lane.sh calls git-rerere-guard.sh ---"

SEED_WARM_LANE="$REPO_ROOT/scripts/seed-warm-lane.sh"

assert "(wiring-lane) scripts/seed-warm-lane.sh exists" \
    test -f "$SEED_WARM_LANE"

assert "(wiring-lane) seed-warm-lane.sh invokes '\"\$…/git-rerere-guard.sh\" arm' (uncommented)" \
    _guard_call_present "$SEED_WARM_LANE"

SEED_NOCALL="$(_without_guard_call "$SEED_WARM_LANE")"

assert "(wiring-lane) negative-control fixture is live (one line commented, nothing else)" \
    _mutation_is_live "$SEED_WARM_LANE" "$SEED_NOCALL"

assert "(wiring-lane) detector FAILS when seed-warm-lane.sh's guard call is commented out" \
    _guard_call_absent "$SEED_NOCALL"

test_summary
