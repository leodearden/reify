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
_dflt_snap="$(mktemp -d)"; _TMPDIRS+=("$_dflt_snap")
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
# scenario: it is what lets `arm` leave the residual 241-entry cache in place
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
    wt_a="$dir-wtA"; wt_b="$dir-wtB"
    _TMPDIRS+=("$wt_a" "$wt_b")
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

# ==============================================================================
# (h) `arm` — idempotently disable rerere in the SHARED local config, preserving
#     rr-cache.  The whole point is that all ~238 lanes inherit one shared
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
# every one of the other ~238 lanes armed while this one reads clean.  A freshly
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
_arm_snap="$(mktemp -d)"; _TMPDIRS+=("$_arm_snap")
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
# lane out of ~239 must not abort everything after it. Contract:
#   0 = disarmed and verified
#   2 = shared config pinned, but an override this run cannot reach survives
#   1 = a genuine failure of this run
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
# A bare MERGE_RR with no .lock sibling is ORDINARY rerere state, present in 41
# of the live store's worktrees today.  Reporting it would make the detector
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
# A lock alongside a real in-flight merge is NOT stale — telling an operator to
# rm it would destroy a live operation's state.  Classify, do not lump.
: > "$LK_B_GITDIR/MERGE_RR.lock"
git -C "$LK_B" rev-parse HEAD > "$LK_B_GITDIR/MERGE_HEAD"

assert "(i-e) lock + MERGE_HEAD -> reported as operation-in-progress" \
    bash -c "bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -qi 'in-progress\|in progress'"

assert "(i-e) the in-progress lane is NOT offered an rm command" \
    bash -c "! bash '$GUARD' scan-locks '$LK_REPO' 2>&1 | grep -q 'rm -f $LK_B_GITDIR/MERGE_RR.lock'"

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
    bash -c "bash '$GUARD' scan-locks '$MC_REPO' 2>&1 | grep -qi 'in-progress\|in progress'"

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
# (wiring) setup-dev.sh must have an UNCOMMENTED call to the guard.
#
# This is a BEHAVIOURAL pin, not a documentation assertion: without it the guard
# is dead code, and the invariant it enforces would silently revert the first
# time anything rewrote the shared .git/config.  Mirrors the (wiring) block in
# tests/infra/test_main_gate_worktree_config.sh.
# ==============================================================================
echo ""
echo "--- (wiring) setup-dev.sh calls git-rerere-guard.sh ---"

SETUP_DEV="$REPO_ROOT/scripts/setup-dev.sh"

assert "(wiring) scripts/setup-dev.sh exists" \
    test -f "$SETUP_DEV"

assert "(wiring) setup-dev.sh calls git-rerere-guard.sh (uncommented)" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -q 'git-rerere-guard.sh'"

# The call must be `arm`, not `check`: setup-dev's job is to make the invariant
# self-healing on the existing setup path, and a bare `check` would merely
# report the drift while leaving the fleet armed.
assert "(wiring) setup-dev.sh invokes the 'arm' subcommand" \
    bash -c "grep -Ev '^[[:space:]]*#' '$SETUP_DEV' | grep -E 'git-rerere-guard\.sh[\"'\''[:space:]]+arm'"

test_summary
