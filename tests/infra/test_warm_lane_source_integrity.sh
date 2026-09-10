#!/usr/bin/env bash
# tests/infra/test_warm_lane_source_integrity.sh
# Hermetic tests for scripts/warm-lane-source-integrity.sh (task 7227).
#
# WHY THIS SUITE EXISTS.
#   esc-7106-5 sighted a tracked source file gone from a warm lane's worktree,
#   with no attribution. `git status --porcelain` prints the same ` D <path>`
#   whether the file is really gone or git is merely answering about a
#   DIFFERENT tree than the one on disk at the lane -- which is exactly what a
#   foreign git view inherited from the environment makes it do. A detector
#   that trusts git's answer fires identically on both and yields another
#   unattributed observation, which is the outcome the script under test
#   exists to prevent.
#
#   So the property pinned here is the DISCRIMINATION: every git-reported
#   worktree deletion is re-checked against the filesystem and bucketed as
#   `deleted` (absent on disk -- the vanished-file signature) or `phantom`
#   (present on disk -- a view artifact), and only the former raises the
#   advisory exit-3 sentinel.
#
#   Deliberately NOT pinned: any claim about WHAT removes a file. Attribution
#   needs a second natural sighting and no test can manufacture one; this suite
#   pins only that the next sighting arrives already classified.
#
# run_detector captures STDOUT, STDERR and RC separately (the idiom of
# tests/infra/test_warm_lane_lock_guard.sh:104-110), because the blocks below
# assert on different ones of the three: stdout must stay exactly one
# machine-readable line, so a diagnostic leaking onto it shows up as a failure
# rather than being swallowed into a merged stream.
#
# Blocks:
#   A — CLEAN: no deletions => exit 0, stdout exactly the one-line
#       `deleted=0 phantom=0` summary, empty stderr; the no-argument form
#       resolves the same lane from the cwd.
#   B — REAL DELETION (the esc-7106-5 signature): exit 3, deleted=N phantom=0,
#       offending paths on stderr -- verbatim, NOT git's C-quoted rendering --
#       and never on stdout.
#   C — PHANTOM DELETION (the foreign-view discriminator, this suite's whole
#       point): git reports ` D` while the path is still on disk => deleted=0
#       phantom=N and exit 0. The block asserts BOTH halves of its own fixture
#       premise first so it cannot decay into a vacuous pass.
#   D — MIXED: one of each in one tree, both measured under a foreign view --
#       so both are reported and NEITHER raises the sentinel (see A3 in the
#       script header). This block previously pinned exit 3 here, which was
#       itself a false sentinel: the path it counted belongs to the poison
#       repo's index, not the lane's.
#   E — NOISE IS NOT COUNTED: untracked, modified-but-present, staged addition,
#       staged deletion (index column, not worktree column) and unmerged
#       conflict entries all leave deleted=0 phantom=0 exit 0.
#   F — FAIL-OPEN / USAGE: a missing lane, a non-worktree lane, a lane whose
#       index has been poisoned into unreadability (task #7106's own
#       mechanism) and a malformed command line all exit 2 with actionable
#       stderr and never report a deletion. A detector that hard-fails an
#       agent session start is worse than one that says nothing -- and one
#       that turns a broken index into a four-figure deletion count is worse
#       than either.
#   G — NON-MUTATION: the lane's source tree is byte-identical (paths, modes,
#       mtimes, sizes, contents) and its porcelain status unchanged across a
#       run that DID find a real deletion -- the run most tempting to "fix".
#   H — WORKTREE-ROOT RELATIVITY: porcelain paths are worktree-root-relative
#       even from a subdirectory, while --lane is taken literally, so a --lane
#       naming a SUBDIRECTORY joins every path against the wrong root and
#       flips the classifier both ways -- a real deletion into a phantom
#       (false all-clear) and a phantom into a real deletion (false sentinel).
#       Pinned as a visible exit-2 wiring error, with the true root and the
#       no-argument-from-a-subdirectory form as controls.
#   I — DEFAULT-FORM RESOLUTION UNDER A POISONED VIEW: the no-argument form is
#       the deployed one (a dark-factory agent-session-start invocation), and
#       inheriting a foreign git view is exactly what happens there -- so it is
#       pinned under the SAME poisoning Block C applies to --lane, which the
#       suite otherwise exercised only under a clean environment. Blocks A and
#       H therefore could not see that resolving the default lane through
#       `git rev-parse --show-toplevel` handed the whole run to the foreign
#       tree. Both measured flips are pinned, plus the clean-foreign case where
#       git reports nothing at all and only the foreign-view notice separates
#       "clean lane" from "lane never measured".
#   J — EACH POISONING VARIABLE ALONE: every other block sets GIT_DIR and
#       GIT_WORK_TREE together, which is the one shape both identity arms see,
#       so the suite could not show either arm was load-bearing. GIT_DIR alone
#       (git compares a FOREIGN index against the lane's files, so foreign-only
#       paths re-stat as genuinely absent -- the loudest false sentinel, and it
#       scales with the foreign repo's tracked-file count) and GIT_WORK_TREE
#       alone (git's gitdir still equals the lane's, so only the toplevel arm
#       sees it), each with a behavioural consequence, plus an unpoisoned
#       control proving the real sentinel survives all of it.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob; classified
# `pool` in run-all-classification.manifest (hermetic: temp-dir git repos only,
# no cargo, no host state, no pool state).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-source-integrity.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-source-integrity.sh hermetic tests (task 7227) ==="

# ──────────────────────────────────────────────────────────────────────────────
# Ambient-env hygiene
# ──────────────────────────────────────────────────────────────────────────────
# The git plumbing vars are scrubbed from the SUITE's own environment before the
# first invocation. Block C deliberately poisons a single child's view with
# GIT_DIR/GIT_WORK_TREE; if the same vars were also ambient here, every OTHER
# block would silently be measuring some foreign tree and the whole suite would
# pass vacuously. Block F's "not a git worktree" assertion is the sharpest
# casualty -- an ambient GIT_DIR makes `git status` succeed anywhere.
# Block C's poisoning is applied on the `bash` command inside a command
# substitution, so it cannot leak back out into this shell.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR GIT_OBJECT_DIRECTORY || true

# ──────────────────────────────────────────────────────────────────────────────
# Shared temp state + cleanup
# ──────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp /tmp/test-warm-lane-source-integrity-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

_mktmpd() {
    local d
    d="$(mktemp -d "/tmp/test-wl-source-integrity-$1-XXXXXX")"
    _TMPDIRS+=("$d")
    printf '%s' "$d"
}

# ── run_detector ──────────────────────────────────────────────────────────────
# Invokes the script under test, capturing OUT (stdout), ERR_OUT (stderr) and
# RC (exit code) as globals -- the three-way split of
# tests/infra/test_warm_lane_lock_guard.sh:104-110.
run_detector() {
    local rc=0
    : > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ── run_detector_in ───────────────────────────────────────────────────────────
# run_detector with the child's cwd set to $1 -- exercises the no-`--lane`
# default, which resolves the lane from the current directory.
run_detector_in() {
    local dir="$1"; shift
    local rc=0
    : > "$ERR_FILE"
    OUT="$(cd "$dir" && bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ── run_detector_poisoned ─────────────────────────────────────────────────────
# run_detector with a FOREIGN git view injected into the child's environment
# only. The assignments sit on the `bash` command inside the command
# substitution rather than prefixing this function call, so they cannot persist
# into the suite's own shell (bash keeps assignments that prefix a *function*
# invocation, which would poison every later block).
run_detector_poisoned() {
    local gitdir="$1" worktree="$2"; shift 2
    local rc=0
    : > "$ERR_FILE"
    OUT="$(GIT_DIR="$gitdir" GIT_WORK_TREE="$worktree" bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ── run_detector_poisoned_in ──────────────────────────────────────────────────
# run_detector_poisoned with the child's cwd set as well -- the combination
# Block I needs, and the one the deployed invocation actually runs in: the
# no-`--lane` default form, from inside a lane, with a foreign view inherited
# from the environment. Same assignment placement, same reason, as
# run_detector_poisoned.
run_detector_poisoned_in() {
    local gitdir="$1" worktree="$2" dir="$3"; shift 3
    local rc=0
    : > "$ERR_FILE"
    OUT="$(cd "$dir" && GIT_DIR="$gitdir" GIT_WORK_TREE="$worktree" bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ── run_detector_gitdir_only / run_detector_worktree_only ─────────────────────
# The two SINGLE-VARIABLE poisoning shapes. run_detector_poisoned sets GIT_DIR
# and GIT_WORK_TREE together, which is only one of the three ways a view can be
# foreign, and it is the one shape BOTH identity arms catch -- so a suite built
# on it alone cannot tell whether either arm is actually load-bearing. Same
# assignment placement, same reason, as run_detector_poisoned.
run_detector_gitdir_only() {
    local gitdir="$1"; shift
    local rc=0
    : > "$ERR_FILE"
    OUT="$(GIT_DIR="$gitdir" bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

run_detector_worktree_only() {
    local worktree="$1"; shift
    local rc=0
    : > "$ERR_FILE"
    OUT="$(GIT_WORK_TREE="$worktree" bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ── string predicates (fork-free; usable as assert checkers) ──────────────────
_has()   { case "$2" in *"$1"*) return 0 ;; *) return 1 ;; esac; }
_lacks() { case "$2" in *"$1"*) return 1 ;; *) return 0 ;; esac; }
_one_line() { test "$(printf '%s\n' "$1" | wc -l)" -eq 1; }
_git_status_fails() { ! git -C "$1" status --porcelain >/dev/null 2>&1; }

# Expected stdout summary for a lane dir. $4 is the `view` field and defaults
# to `lane`; every poisoned run must pass `foreign`, so a block that poisons the
# environment and forgets to say so fails loudly rather than matching a
# lane-view line.
_summary() {
    printf 'source-integrity: deleted=%s phantom=%s lane=%s view=%s' \
        "$1" "$2" "$(basename "$3")" "${4:-lane}"
}

# ── fixture builders ──────────────────────────────────────────────────────────
# A throwaway repo under /tmp: no global core.hooksPath is configured on this
# host and a fresh `git init` uses its own empty .git/hooks, so reify's hooks
# never fire on these fixtures.
_git_q() { git -c user.name=t -c user.email=t@t -c commit.gpgsign=false "$@"; }

# _mk_repo <dir> <relpath>...  — create and commit each relpath with its own
# path as content. Path set deliberately spans the porcelain quoting edges: a
# nested dir, a name with a space, and a dotfile.
_mk_repo() {
    local d="$1"; shift
    mkdir -p "$d"
    _git_q -C "$d" init -q -b main
    local p
    for p in "$@"; do
        mkdir -p "$d/$(dirname "$p")"
        printf 'content of %s\n' "$p" > "$d/$p"
    done
    _git_q -C "$d" add -A
    _git_q -C "$d" commit -q -m init
}

# _snapshot_lane <dir> — deterministic text image of the lane's SOURCE tree
# plus its porcelain status. .git/ is excluded on purpose: `git status`
# legitimately refreshes the index's cached stat data, and the claim under test
# is that the SOURCE tree is untouched, not that git never bookkeeps.
_snapshot_lane() {
    local lane="$1"
    (
        cd "$lane"
        find . -path ./.git -prune -o -mindepth 1 -print | LC_ALL=C sort |
        while IFS= read -r p; do
            printf '%s\t%s' "$p" "$(stat -c '%f:%Y:%s' "$p")"
            if [ -f "$p" ] && [ ! -L "$p" ]; then
                printf '\t%s' "$(sha256sum < "$p" | cut -d' ' -f1)"
            fi
            printf '\n'
        done
    )
    git -C "$lane" status --porcelain
}

# ──────────────────────────────────────────────────────────────────────────────
# Block A — CLEAN
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: clean lane ---"

A_LANE="$(_mktmpd A)/lane"
_mk_repo "$A_LANE" a.txt src/lib.rs 'with space.txt' .hidden nested/deep/x.txt

run_detector --lane "$A_LANE"
assert "A1: a clean lane exits 0" test "$RC" -eq 0
assert "A2: stdout is exactly the one-line zero summary" \
    test "$OUT" = "$(_summary 0 0 "$A_LANE")"
assert "A3: stdout really is a single line" _one_line "$OUT"
assert "A4: a clean lane says nothing on stderr" test -z "$ERR_OUT"

# The no-argument form must resolve the same lane from the cwd, or the default
# path is dead code that only the --lane form ever exercises.
run_detector_in "$A_LANE"
assert "A5: the no-argument form resolves the lane from the cwd (exit 0)" \
    test "$RC" -eq 0
assert "A6: ...and produces the identical summary line" \
    test "$OUT" = "$(_summary 0 0 "$A_LANE")"

# Run from a SUBDIRECTORY: porcelain paths are repo-root-relative, so a
# detector that stat-ed them against the cwd instead of the worktree root would
# mis-bucket every entry here.
run_detector_in "$A_LANE/nested/deep"
assert "A7: the no-argument form works from a subdirectory too (exit 0)" \
    test "$RC" -eq 0
assert "A8: ...and still names the worktree root as the lane" \
    test "$OUT" = "$(_summary 0 0 "$A_LANE")"

# ──────────────────────────────────────────────────────────────────────────────
# Block B — REAL DELETION (the esc-7106-5 signature)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block B: real deletion ---"

B_LANE="$(_mktmpd B)/lane"
_mk_repo "$B_LANE" a.txt src/lib.rs 'with space.txt'
rm "$B_LANE/src/lib.rs"

run_detector --lane "$B_LANE"
assert "B1: a real deletion raises the advisory sentinel (exit 3)" test "$RC" -eq 3
assert "B2: it is counted as deleted, not phantom" \
    test "$OUT" = "$(_summary 1 0 "$B_LANE")"
assert "B3: stdout stays a single machine-readable line" _one_line "$OUT"
assert "B4: the offending path is named on stderr" _has 'src/lib.rs' "$ERR_OUT"
assert "B5: ...and never leaks onto stdout" _lacks 'src/lib.rs' "$OUT"

# Paths git C-quotes in porcelain v1 (`"with space.txt"`, `"na\303\257ve.txt"`)
# must reach the operator verbatim. A detector that reports git's rendering
# hands over a path that cannot be pasted into a shell or an ls.
B_QUOTED="$(_mktmpd Bq)/lane"
_mk_repo "$B_QUOTED" a.txt 'with space.txt' 'naïve.txt'
rm "$B_QUOTED/with space.txt" "$B_QUOTED/naïve.txt"

run_detector --lane "$B_QUOTED"
assert "B6: both quoting-edge deletions are counted (exit 3, deleted=2)" \
    test "$OUT" = "$(_summary 2 0 "$B_QUOTED")"
assert "B7: the spaced path is reported verbatim on stderr" \
    _has 'with space.txt' "$ERR_OUT"
assert "B8: ...not in git's C-quoted rendering" \
    _lacks '"with space.txt"' "$ERR_OUT"
assert "B9: the non-ASCII path is reported verbatim, not octal-escaped" \
    _lacks 'na\303\257ve.txt' "$ERR_OUT"
assert "B10: ...and the real UTF-8 name is present" _has 'naïve.txt' "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block C — PHANTOM DELETION (the #7106 discriminator)
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block C: phantom deletion ---"

# Fixture: two repos with the same path set. The files are removed from the
# POISON worktree only; the lane's copies are untouched. Running the detector
# with GIT_DIR/GIT_WORK_TREE aimed at the poison repo makes git answer about a
# tree that is not the one on disk at the lane, so it reports deletions for
# paths that are demonstrably present there. That is the same class of defect
# as the git-environment leaks scripts/lib_git_env_scrub.sh documents -- a
# foreign view inherited from the environment -- and the detector is specified
# NOT to scrub such a view but to observe it and classify it, because
# classifying the agent's own view is the point.
C_ROOT="$(_mktmpd C)"
C_LANE="$C_ROOT/lane"
C_POISON="$C_ROOT/poison"
_mk_repo "$C_LANE"   a.txt src/lib.rs 'with space.txt'
_mk_repo "$C_POISON" a.txt src/lib.rs 'with space.txt'
rm "$C_POISON/src/lib.rs" "$C_POISON/with space.txt"

# Assert the fixture's own premise, both halves, before asserting on the
# detector. Without this the block degrades silently into a vacuous pass the
# moment the construction stops producing a ` D`.
C_POISONED_STATUS="$(GIT_DIR="$C_POISON/.git" GIT_WORK_TREE="$C_POISON" git -C "$C_LANE" status --porcelain)"
assert "C0a: FIXTURE — the poisoned view really reports src/lib.rs as worktree-deleted" \
    _has ' D src/lib.rs' "$C_POISONED_STATUS"
assert "C0b: FIXTURE — ...while the lane's copy is really still on disk" \
    test -f "$C_LANE/src/lib.rs"
assert "C0c: FIXTURE — same for the quoted path" \
    _has ' D "with space.txt"' "$C_POISONED_STATUS"
assert "C0d: FIXTURE — ...and its lane copy is on disk too" \
    test -f "$C_LANE/with space.txt"

run_detector_poisoned "$C_POISON/.git" "$C_POISON" --lane "$C_LANE"
assert "C1: an index artifact does NOT raise the sentinel (exit 0)" test "$RC" -eq 0
assert "C2: both entries are bucketed phantom, none deleted" \
    test "$OUT" = "$(_summary 0 2 "$C_LANE" foreign)"
assert "C3: stdout stays a single machine-readable line" _one_line "$OUT"
assert "C4: the phantom paths are still reported on stderr" \
    _has 'src/lib.rs' "$ERR_OUT"
assert "C5: ...tagged as phantom so the class is legible without re-deriving it" \
    _has 'phantom' "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block D — MIXED
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block D: mixed real + phantom ---"

# The poison repo tracks both here.txt and gone.txt and has both removed from
# its worktree; the lane has only here.txt. One reported deletion is therefore
# genuinely absent from the lane and the other is genuinely present.
D_ROOT="$(_mktmpd D)"
D_LANE="$D_ROOT/lane"
D_POISON="$D_ROOT/poison"
_mk_repo "$D_LANE"   a.txt here.txt
_mk_repo "$D_POISON" a.txt here.txt gone.txt
rm "$D_POISON/here.txt" "$D_POISON/gone.txt"

assert "D0: FIXTURE — here.txt is on disk in the lane" test -f "$D_LANE/here.txt"
assert "D0: FIXTURE — gone.txt is not" test ! -e "$D_LANE/gone.txt"

run_detector_poisoned "$D_POISON/.git" "$D_POISON" --lane "$D_LANE"
assert "D1: one of each is reported" test "$OUT" = "$(_summary 1 1 "$D_LANE" foreign)"
# D2 CHANGED (task 7227, review round 2), and the change is the point of the
# block now. This fixture never showed a lane defect: gone.txt is a path the
# POISON repo tracks and the lane does not, so "absent under the lane" is its
# mundane resting state, not a vanishing. The suite previously pinned exit 3
# here -- the same false sentinel the GIT_DIR-only shape produces at scale in
# Block J, arriving by a different variable. A sentinel is a claim about THIS
# lane, so it may only be raised from evidence measured through this lane's own
# repository and worktree; under a foreign view the count is still reported, and
# `view=foreign` is what says so.
assert "D2: a foreign view's deletion raises NO sentinel (exit 0, not 3)" test "$RC" -eq 0
assert "D3: the unattributable path is still named on stderr" _has 'gone.txt' "$ERR_OUT"
assert "D4: so is the phantom" _has 'here.txt' "$ERR_OUT"
assert "D5: ...and it is not labelled as this lane's evidence" \
    _lacks 'do NOT restore' "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block E — NOISE IS NOT COUNTED
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block E: noise is not counted ---"

E_LANE="$(_mktmpd E)/lane"
_mk_repo "$E_LANE" a.txt src/lib.rs tracked-then-rm.txt
printf 'junk\n'     > "$E_LANE/untracked.txt"          # ?? — untracked
printf 'modified\n' > "$E_LANE/a.txt"                  #  M — present, changed
printf 'added\n'    > "$E_LANE/staged-add.txt"
_git_q -C "$E_LANE" add staged-add.txt                 # A  — staged addition
_git_q -C "$E_LANE" rm -q tracked-then-rm.txt          # D  — INDEX column, an
                                                       # intentional removal, not
                                                       # the vanishing signature

run_detector --lane "$E_LANE"
assert "E1: none of the noise classes is counted" \
    test "$OUT" = "$(_summary 0 0 "$E_LANE")"
assert "E2: and none of them raises the sentinel (exit 0)" test "$RC" -eq 0
assert "E3: FIXTURE — the noise really is in the porcelain output" \
    _has 'staged-add.txt' "$(git -C "$E_LANE" status --porcelain)"
assert "E4: FIXTURE — the staged removal really is an X-column D" \
    _has 'D  tracked-then-rm.txt' "$(git -C "$E_LANE" status --porcelain)"

# Unmerged entries reuse the XY columns for conflict state, so a modify/delete
# conflict prints `UD <path>` with the path still on disk. Reading that Y=D as
# a worktree deletion would inflate every conflicted lane's report -- and its
# `DD` sibling would raise a false sentinel outright.
E_CONFLICT="$(_mktmpd Ec)/lane"
_mk_repo "$E_CONFLICT" a.txt c.txt
_git_q -C "$E_CONFLICT" checkout -q -b side
_git_q -C "$E_CONFLICT" rm -q c.txt
_git_q -C "$E_CONFLICT" commit -q -m "delete on side"
_git_q -C "$E_CONFLICT" checkout -q main
printf 'changed\n' > "$E_CONFLICT/c.txt"
_git_q -C "$E_CONFLICT" add -A
_git_q -C "$E_CONFLICT" commit -q -m "modify on main"
_git_q -C "$E_CONFLICT" merge side >/dev/null 2>&1 || true

assert "E5: FIXTURE — the conflict really prints a Y-column D" \
    _has 'UD c.txt' "$(git -C "$E_CONFLICT" status --porcelain)"
assert "E5: FIXTURE — ...with the file still on disk" test -f "$E_CONFLICT/c.txt"

run_detector --lane "$E_CONFLICT"
assert "E6: an unmerged conflict entry is not a deletion of either kind" \
    test "$OUT" = "$(_summary 0 0 "$E_CONFLICT")"
assert "E7: ...and does not raise the sentinel (exit 0)" test "$RC" -eq 0

# ──────────────────────────────────────────────────────────────────────────────
# Block F — FAIL-OPEN / USAGE
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block F: fail-open and usage ---"

F_ROOT="$(_mktmpd F)"
F_NOT_A_REPO="$F_ROOT/plain-dir"
mkdir -p "$F_NOT_A_REPO"
printf 'x\n' > "$F_NOT_A_REPO/file.txt"

run_detector --lane "$F_ROOT/does-not-exist"
assert "F1: a nonexistent --lane is a wiring error (exit 2)" test "$RC" -eq 2
assert "F2: ...and is actionable on stderr" test -n "$ERR_OUT"
assert "F3: ...and never reports a deletion" _lacks 'deleted=' "$OUT"

run_detector --lane "$F_NOT_A_REPO"
assert "F4: a non-worktree --lane is a wiring error (exit 2)" test "$RC" -eq 2
assert "F5: ...and is actionable on stderr" test -n "$ERR_OUT"
assert "F6: ...and never reports a deletion" _lacks 'deleted=' "$OUT"
assert "F7: ...and does not create anything in the directory it was pointed at" \
    test ! -e "$F_NOT_A_REPO/.git"

run_detector --lane "$A_LANE" --no-such-flag
assert "F8: an unknown flag is a usage error (exit 2)" test "$RC" -eq 2
assert "F9: ...and names the offending flag on stderr" _has 'no-such-flag' "$ERR_OUT"

run_detector --lane
assert "F10: --lane with no value is a usage error (exit 2)" test "$RC" -eq 2
assert "F11: ...and is actionable on stderr" test -n "$ERR_OUT"

run_detector --help
assert "F12: --help exits 0" test "$RC" -eq 0
assert "F13: ...and documents the exit codes" _has 'Exit codes' "$OUT$ERR_OUT"

# Task #7106's OWN mechanism, per scripts/lib_git_env_scrub.sh: an unscrubbed
# GIT_INDEX_FILE lets one repo's `git add -A` overwrite another repo's index.
# Measured on git 2.43.0 — the victim's index then names blobs living in the
# FOREIGN object store, so `git status` in the victim is fatal. Reporting the
# entries such an index makes git call deleted would be the loudest possible
# false sentinel (#7106 measured 2010075 of them), so this must degrade to the
# wiring-error exit instead. `env` runs the poisoning assignment on an EXTERNAL
# command, which cannot leak back into this shell the way a prefix on a
# function call would.
F_POISON_ROOT="$(_mktmpd Fp)"
F_VICTIM="$F_POISON_ROOT/victim"
F_FOREIGN="$F_POISON_ROOT/foreign"
_mk_repo "$F_VICTIM"  a.txt src/lib.rs
_mk_repo "$F_FOREIGN" foreign-only.txt
env GIT_INDEX_FILE="$F_VICTIM/.git/index" \
    git -c user.name=t -c user.email=t@t -C "$F_FOREIGN" add -A

assert "F14: FIXTURE — the poisoned index really makes git status fatal in the victim" \
    _git_status_fails "$F_VICTIM"
assert "F15: FIXTURE — ...while every one of the victim's files is still on disk" \
    test -f "$F_VICTIM/src/lib.rs"

run_detector --lane "$F_VICTIM"
assert "F16: an unreadable index is a wiring error (exit 2), not a false sentinel" \
    test "$RC" -eq 2
assert "F17: ...and no deletion count is reported at all" _lacks 'deleted=' "$OUT"
assert "F18: ...and git's own error is surfaced rather than swallowed" \
    _has 'git:' "$ERR_OUT"

# ──────────────────────────────────────────────────────────────────────────────
# Block G — NON-MUTATION
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block G: non-mutation ---"

# Deliberately run against a lane that HAS a real deletion: that is the run a
# future "helpful" edit is most likely to turn into a `git checkout --` restore,
# which would destroy the very evidence a second sighting needs.
G_LANE="$(_mktmpd G)/lane"
_mk_repo "$G_LANE" a.txt src/lib.rs 'with space.txt' .hidden nested/deep/x.txt
rm "$G_LANE/src/lib.rs"

G_BEFORE="$(_snapshot_lane "$G_LANE")"
run_detector --lane "$G_LANE"
G_AFTER="$(_snapshot_lane "$G_LANE")"

assert "G0: FIXTURE — the run really did find the deletion (exit 3)" test "$RC" -eq 3
assert "G1: the lane's source tree is byte-identical across the run" \
    test "$G_BEFORE" = "$G_AFTER"
assert "G2: the deleted file was NOT restored" test ! -e "$G_LANE/src/lib.rs"
assert "G3: nothing was staged" \
    test -z "$(git -C "$G_LANE" diff --cached --name-only)"

# ──────────────────────────────────────────────────────────────────────────────
# Block H — WORKTREE-ROOT RELATIVITY
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block H: worktree-root relativity ---"

# `git status --porcelain` emits WORKTREE-ROOT-relative paths even when it is
# invoked from a subdirectory -- measured on git 2.43.0: `git -C <root>/sub
# status --porcelain` prints ` D sub/x.txt`, not ` D x.txt`. But `--lane` is
# taken literally (deliberately -- see the script's header) and every candidate
# is stat-ed as `$LANE/$_path`, so pointing `--lane` at a SUBDIRECTORY joins
# every entry against the wrong root and flips the classifier in BOTH
# directions. Both flips were reproduced against the pre-guard script:
#
#   FORWARD (false all-clear, the worse one) -- a tracked file that really is
#     gone is re-stat-ed one level too deep, lands on a DIFFERENT real file
#     with a colliding leaf name, and is reported `phantom=1` exit 0. The
#     esc-7106-5 signature this whole script exists to catch, silently
#     relabelled a view artifact and the sentinel suppressed.
#   REVERSE (false sentinel) -- a genuine phantom whose root-relative path has
#     no counterpart under the subdirectory is reported `deleted=1` exit 3.
#
# The trap is sharpened by an asymmetry this suite already pins: A7/A8 run the
# NO-ARGUMENT form from `$A_LANE/nested/deep` and it is correct there, because
# that form walks up to the lane root instead of taking the cwd. So an operator
# who learns "run it from inside the lane" and then switches to `--lane .` from
# that same subdirectory gets the opposite behaviour. The header's exit-2
# contract already claims this class ("a mis-wired invocation is visible");
# this block is what makes the claim true. Block F covers only a nonexistent
# directory and a plain non-repo one.

H_ROOT="$(_mktmpd H)"
H_LANE="$H_ROOT/lane"
H_POISON="$H_ROOT/poison"

# The path set is the fixture's whole point: `sub/x.txt` and `sub/sub/x.txt`
# share a leaf name one level apart, so the bad join lands on a REAL file
# rather than merely missing. A fixture without that collision would produce
# the right answer by accident and pin nothing.
_mk_repo "$H_LANE"   a.txt sub/x.txt sub/sub/x.txt sub/y.txt
_mk_repo "$H_POISON" a.txt sub/x.txt sub/sub/x.txt sub/y.txt
rm "$H_LANE/sub/x.txt"     # the real deletion, at root-relative path sub/x.txt
rm "$H_POISON/sub/y.txt"   # makes the poisoned view report sub/y.txt deleted

# Assert the fixture's own premise before asserting on the detector (the
# C0a-C0d convention), so neither flip can decay into a vacuous pass.
H_STATUS="$(git -C "$H_LANE" status --porcelain)"
assert "H0a: FIXTURE — the lane really reports a root-relative ' D sub/x.txt'" \
    _has ' D sub/x.txt' "$H_STATUS"
assert "H0b: FIXTURE — ...and that path really is absent on disk" \
    test ! -e "$H_LANE/sub/x.txt"
assert "H0c: FIXTURE — ...while the colliding leaf one level deeper IS present" \
    test -f "$H_LANE/sub/sub/x.txt"

H_POISONED_STATUS="$(GIT_DIR="$H_POISON/.git" GIT_WORK_TREE="$H_POISON" git -C "$H_LANE" status --porcelain)"
assert "H0d: FIXTURE — the poisoned view reports ' D sub/y.txt'" \
    _has ' D sub/y.txt' "$H_POISONED_STATUS"
assert "H0e: FIXTURE — ...whose lane copy is present, so it is a genuine phantom" \
    test -f "$H_LANE/sub/y.txt"
assert "H0f: FIXTURE — ...and has no counterpart under the subdirectory" \
    test ! -e "$H_LANE/sub/sub/y.txt"

# H1-H5: the mis-wiring must be a visible exit-2 error, not either flip.
run_detector --lane "$H_LANE/sub"
assert "H1: a --lane pointing at a subdirectory is a wiring error (exit 2)" \
    test "$RC" -eq 2
assert "H2a: ...and names the lane-root requirement on stderr" \
    _has 'root' "$ERR_OUT"
assert "H2b: ...and names the rejected path, so the mis-wiring is fixable" \
    _has "$H_LANE/sub" "$ERR_OUT"
assert "H3: ...and emits no classification line at all (not a classified run)" \
    _lacks 'deleted=' "$OUT"
assert "H4: ...so the real deletion is NOT relabelled a phantom (false all-clear)" \
    _lacks 'phantom=1' "$OUT"

run_detector_poisoned "$H_POISON/.git" "$H_POISON" --lane "$H_LANE/sub"
assert "H5a: the reverse flip is refused too (exit 2, not the exit-3 sentinel)" \
    test "$RC" -eq 2
assert "H5b: ...so a genuine phantom is never reported as a real deletion" \
    _lacks 'deleted=1' "$OUT"

# H6/H7: controls. Without them a guard that rejected EVERY lane would pass
# H1-H5 trivially, and the intended division of labour -- `--lane` demands the
# root, the default form derives it -- would go unstated.
run_detector --lane "$H_LANE"
assert "H6a: POSITIVE CONTROL — the true lane root still classifies (exit 3)" \
    test "$RC" -eq 3
assert "H6b: ...as a real deletion, on the very fixture the subdir form flipped" \
    test "$OUT" = "$(_summary 1 0 "$H_LANE")"

run_detector_in "$H_LANE/sub"
assert "H7a: ASYMMETRY CONTROL — the no-argument form from that same subdirectory" \
    test "$RC" -eq 3
assert "H7b: ...still walks up to the root and is correct" \
    test "$OUT" = "$(_summary 1 0 "$H_LANE")"

assert "H8: the rejected subdirectory is untouched — nothing was created in it" \
    test ! -e "$H_LANE/sub/.git"

# ──────────────────────────────────────────────────────────────────────────────
# Block I — DEFAULT-FORM RESOLUTION UNDER A POISONED VIEW
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block I: the no-argument form under a foreign git view ---"

# The default form is the DEPLOYED one -- a dark-factory agent-session-start
# invocation, where environment inheritance is exactly what happens -- yet
# Blocks A and H exercised it only under a clean environment while Blocks C, D
# and H poisoned only the `--lane` form. That gap hid a real defect, measured
# on this host against the pre-fix script, which resolved the default lane with
# `git rev-parse --show-toplevel`: rev-parse honours the inherited view, so
# LANE became the FOREIGN worktree and the root guard, the on-disk re-stat and
# the `lane=` field all described that tree instead.
#
#   I1/I2 — a lane whose own tracked file really is gone must still be reported
#     against THE LANE. Pre-fix this printed `lane=poison`: the right counts
#     attributed to the wrong tree, which is an unattributable report -- the
#     one outcome this whole script exists to prevent.
#   I3 — the reverse: a foreign tree's deletion must never be charged to a
#     clean lane. Pre-fix this printed `deleted=1 lane=poison` and exited 3,
#     a false sentinel naming a lane with nothing wrong with it.
#   I4 — the sharpest one: when the foreign tree is CLEAN, git reports nothing
#     at all, so both buckets are empty and the summary is indistinguishable
#     from a healthy lane. Pre-fix that read `deleted=0 phantom=0 lane=poison`
#     for a lane whose file really was gone -- the esc-7106-5 signature
#     silently suppressed. Correct resolution alone does NOT restore the
#     counts here (git was asked about the other tree and answered honestly
#     about it), so what is pinned is that the run says so.
I_ROOT="$(_mktmpd I)"
I_LANE="$I_ROOT/lane"
I_POISON="$I_ROOT/poison"
I_CLEAN="$I_ROOT/cleanview"
_mk_repo "$I_LANE"   a.txt sub/a.txt
_mk_repo "$I_POISON" a.txt sub/a.txt
_mk_repo "$I_CLEAN"  a.txt sub/a.txt
rm "$I_LANE/sub/a.txt" "$I_POISON/sub/a.txt"

# Fixture premises first (the C0a-C0d convention), so no assertion below can
# decay into a vacuous pass.
I_POISONED_STATUS="$(GIT_DIR="$I_POISON/.git" GIT_WORK_TREE="$I_POISON" git -C "$I_LANE" status --porcelain)"
assert "I0a: FIXTURE — the poisoned view reports ' D sub/a.txt'" \
    _has ' D sub/a.txt' "$I_POISONED_STATUS"
assert "I0b: FIXTURE — ...and the lane's own copy really is absent" \
    test ! -e "$I_LANE/sub/a.txt"
assert "I0c: FIXTURE — the lane and the poison tree have distinct basenames" \
    test "$(basename "$I_LANE")" != "$(basename "$I_POISON")"

run_detector_poisoned_in "$I_POISON/.git" "$I_POISON" "$I_LANE"
assert "I1a: the no-argument form raises no sentinel under a foreign view (exit 0)" \
    test "$RC" -eq 0
assert "I1b: ...and names THE LANE, not the inherited tree, marked view=foreign" \
    test "$OUT" = "$(_summary 1 0 "$I_LANE" foreign)"
assert "I1c: ...so the foreign tree is never named as the lane" \
    _lacks "lane=$(basename "$I_POISON")" "$OUT"

# From a subdirectory as well: the walk-up must reach the lane ROOT, not stop
# at the cwd, or every root-relative porcelain path is joined one level too
# deep -- the Block H flip, arriving by the other branch.
run_detector_poisoned_in "$I_POISON/.git" "$I_POISON" "$I_LANE/sub"
assert "I2a: ...and from a subdirectory of the lane too (exit 0)" test "$RC" -eq 0
assert "I2b: ...still naming the lane root, not the subdirectory or the view" \
    test "$OUT" = "$(_summary 1 0 "$I_LANE" foreign)"

# REVERSE: a clean lane under a view that has a deletion. The deletion belongs
# to the foreign tree, so the lane must come back with no sentinel -- and the
# entry classified phantom, which is the proof that scrubbing was confined to
# resolution and the view itself is still being observed.
I_CLEANLANE="$I_ROOT/cleanlane"
_mk_repo "$I_CLEANLANE" a.txt sub/a.txt
assert "I3a: FIXTURE — the clean lane really has its copy on disk" \
    test -f "$I_CLEANLANE/sub/a.txt"

run_detector_poisoned_in "$I_POISON/.git" "$I_POISON" "$I_CLEANLANE"
assert "I3b: a foreign tree's deletion never becomes the lane's sentinel (exit 0)" \
    test "$RC" -eq 0
assert "I3c: ...it is classified phantom against the correctly-resolved lane" \
    test "$OUT" = "$(_summary 0 1 "$I_CLEANLANE" foreign)"
assert "I3d: ...so the poisoned view is still OBSERVED, not scrubbed away" \
    _has 'phantom' "$ERR_OUT"

# CLEAN FOREIGN VIEW: git has nothing to report, so the counts cannot carry the
# signal. The run must still say whose tree it answered about, or a session
# start under a poisoned environment reads as a clean bill of health.
run_detector_poisoned_in "$I_CLEAN/.git" "$I_CLEAN" "$I_LANE"
assert "I4a: a clean foreign view yields a zero summary — against the LANE's name" \
    test "$OUT" = "$(_summary 0 0 "$I_LANE" foreign)"
assert "I4b: ...and the run is not silent: the foreign tree is named on stderr" \
    _has "$I_CLEAN" "$ERR_OUT"
assert "I4c: ...stated as a different tree, so it is legible without re-deriving" \
    _has 'NOT this lane' "$ERR_OUT"
assert "I4d: ...and explicitly not an all-clear for this lane" \
    _has 'NOT an all-clear' "$ERR_OUT"
assert "I4e: stdout stays exactly one machine-readable line regardless" _one_line "$OUT"

# CONTROL: without a foreign view there is no notice at all, or the notice
# would be noise on every healthy run and A4's silence claim would be a lie.
run_detector_in "$I_CLEANLANE"
assert "I5a: CONTROL — an unpoisoned run of the same form is clean (exit 0)" \
    test "$RC" -eq 0
assert "I5b: ...and emits no foreign-view notice" _lacks 'NOT this lane' "$ERR_OUT"

assert "I6: NON-MUTATION — the lane's absent file was not restored by any of it" \
    test ! -e "$I_LANE/sub/a.txt"

# ──────────────────────────────────────────────────────────────────────────────
# Block J — EACH POISONING VARIABLE ALONE
# ──────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block J: GIT_DIR alone and GIT_WORK_TREE alone ---"

# Every other block sets GIT_DIR and GIT_WORK_TREE TOGETHER. Measured on this
# host, git 2.43.0, `git -C <lane> rev-parse --show-toplevel --absolute-git-dir`:
#
#   GIT_DIR + GIT_WORK_TREE  toplevel=poison   gitdir=poison
#   GIT_DIR only             toplevel=LANE     gitdir=poison
#   GIT_WORK_TREE only       toplevel=poison   gitdir=LANE
#
# So the paired shape is the only one BOTH identity arms see, and a suite built
# on it cannot show either arm is load-bearing. J1 is the GIT_DIR-only shape --
# invisible to a toplevel-equality check -- and J2 the GIT_WORK_TREE-only shape,
# invisible to a gitdir-equality check. Deleting either arm turns exactly one of
# them red.
#
# J1 is also the severe one, and the reason the sentinel is now withheld rather
# than merely annotated. With GIT_DIR alone git compares a FOREIGN INDEX against
# the LANE's files, so every path the foreign repo tracks and this lane does not
# is reported ` D` and re-stats as genuinely absent here. The on-disk
# discriminator -- the whole script -- cannot separate that from a real vanish,
# because the paths really are missing. Pre-fix this printed `deleted=2 lane=lane`
# and exited 3: an exit-3 sentinel naming the lane, listing files that were never
# the lane's, scaling with the FOREIGN repo's tracked-file count. It is the
# unattributable sighting this script exists to prevent, wearing the script's own
# alarm. An inherited GIT_DIR naming another lane of the shared .git store would
# have rendered it as a plausible set of reify source paths.
J_ROOT="$(_mktmpd J)"
J_LANE="$J_ROOT/lane"
J_POISON="$J_ROOT/poison"
_mk_repo "$J_LANE"   a.txt
_mk_repo "$J_POISON" src/foreign1.rs src/foreign2.rs

assert "J0a: FIXTURE — the lane's own file is present and it tracks nothing else" \
    test -f "$J_LANE/a.txt"
assert "J0b: FIXTURE — the foreign-only paths are absent under the lane, so the" \
    test ! -e "$J_LANE/src/foreign1.rs"
assert "J0c: FIXTURE — ...on-disk re-stat cannot tell them from a real vanish" \
    test ! -e "$J_LANE/src/foreign2.rs"

J_GITDIR_STATUS="$(GIT_DIR="$J_POISON/.git" git -C "$J_LANE" status --porcelain)"
assert "J0d: FIXTURE — GIT_DIR alone really makes git report the foreign paths ' D'" \
    _has ' D src/foreign1.rs' "$J_GITDIR_STATUS"

run_detector_gitdir_only "$J_POISON/.git" --lane "$J_LANE"
assert "J1a: GIT_DIR alone raises NO sentinel (exit 0) — the false alarm is gone" \
    test "$RC" -eq 0
assert "J1b: ...the run is marked view=foreign, so exit 0 is not read as all-clear" \
    test "$OUT" = "$(_summary 2 0 "$J_LANE" foreign)"
assert "J1c: ...git's toplevel equals the lane here, so only the GITDIR arm can see it" \
    test "$(GIT_DIR="$J_POISON/.git" git -C "$J_LANE" rev-parse --show-toplevel)" = "$J_LANE"
assert "J1d: ...and the foreign REPOSITORY is named, not merely a foreign tree" \
    _has 'different repository' "$ERR_OUT"
assert "J1e: ...the paths are tagged unattributable, not asserted as lane evidence" \
    _has 'unattributable: src/foreign1.rs' "$ERR_OUT"
assert "J1f: ...so the 'do NOT restore, this is evidence' hint is withheld" \
    _lacks 'do NOT restore' "$ERR_OUT"

run_detector_worktree_only "$J_POISON" --lane "$J_LANE"
assert "J2a: GIT_WORK_TREE alone is caught too (exit 0, view=foreign)" \
    test "$RC" -eq 0
assert "J2b: ...as a foreign WORKTREE — the arm the gitdir check cannot see" \
    _has 'different worktree' "$ERR_OUT"
assert "J2c: ...git's gitdir equals the lane's own here, hence the second arm" \
    test "$(GIT_WORK_TREE="$J_POISON" git -C "$J_LANE" rev-parse --absolute-git-dir)" = "$J_LANE/.git"
assert "J2d: ...and the lane is still the one named" _has "lane=$(basename "$J_LANE")" "$OUT"

# J2e/J2f give the worktree arm a BEHAVIOURAL consequence, not just a wording
# one: J2a-J2d survive its removal except for the message text, so on their own
# they would let the arm rot into a label. Here the lane has a real deletion,
# and GIT_WORK_TREE alone makes git list every lane-tracked path missing from
# the FOREIGN worktree; the re-stat then finds sub/b.txt genuinely absent.
# Without the arm that is exit 3.
#
# Stated plainly because it is the one uncomfortable edge of the uniform rule:
# in THIS shape the positive is actually sound -- the lane's own index tracks
# sub/b.txt and it really is gone -- so withholding the sentinel withholds a
# true one. It is withheld anyway, because the entry SET is dictated by the
# foreign worktree's contents rather than the lane's: had the poison repo
# happened to contain sub/b.txt, the vanished file would not have been listed
# at all. A rule that raised the sentinel from a set chosen by an unrelated
# repository would be unreliable in the miss direction while looking
# authoritative, and carving out this one axis would trade one uniform
# invariant for a per-variable exception. Nothing is lost permanently: the
# paths are still listed, view=foreign says why, and the scrubbed re-run the
# notice asks for produces the true sentinel.
J_WT_LANE="$J_ROOT/wtlane"
J_WT_POISON="$J_ROOT/wtpoison"
_mk_repo "$J_WT_LANE"   a.txt sub/b.txt
_mk_repo "$J_WT_POISON" other.txt
rm "$J_WT_LANE/sub/b.txt"
assert "J2e: FIXTURE — the lane's own tracked file really is gone" \
    test ! -e "$J_WT_LANE/sub/b.txt"

run_detector_worktree_only "$J_WT_POISON" --lane "$J_WT_LANE"
assert "J2f: GIT_WORK_TREE alone withholds the sentinel behaviourally (exit 0, not 3)" \
    test "$RC" -eq 0
assert "J2g: ...the path is still listed and the view marked, so nothing is hidden" \
    test "$OUT" = "$(_summary 1 1 "$J_WT_LANE" foreign)"
assert "J2h: ...and a scrubbed re-run of the same lane DOES raise it (exit 3)" \
    bash -c 'bash "$0" --lane "$1" >/dev/null 2>&1; test $? -eq 3' "$SCRIPT" "$J_WT_LANE"

# CONTROL: the sentinel must survive everything above. A fix that suppressed it
# broadly would pass every assertion in this block and destroy the script.
J_REAL="$J_ROOT/reallane"
_mk_repo "$J_REAL" a.txt sub/b.txt
rm "$J_REAL/sub/b.txt"
run_detector --lane "$J_REAL"
assert "J3a: CONTROL — an unpoisoned real deletion still raises the sentinel (exit 3)" \
    test "$RC" -eq 3
assert "J3b: ...marked view=lane, the only view a sentinel may be raised from" \
    test "$OUT" = "$(_summary 1 0 "$J_REAL" lane)"
assert "J3c: ...and it still carries the evidence-preservation hint" \
    _has 'do NOT restore' "$ERR_OUT"

test_summary
