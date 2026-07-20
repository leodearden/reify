#!/usr/bin/env bash
# tests/infra/test_run_all_content_skip.sh — gate test for the merge-tier
# content-addressed per-member SKIP engine in tests/infra/run_all.sh
# (task 5273, merge-gate-riders PRD §4 rider γ).
#
# The skip engine drops a drift-guard pool member from the merge-tier run
# when its declared tracked-file closure (run-all-skip-closures.manifest) is
# byte-identical (git tree compare) to its last-executed-green main sha, so
# an unchanged member is not re-run every merge. Ships PRODUCTION-INERT:
# active ONLY when REIFY_RUN_ALL_CONTENT_SKIP=1 AND the inbound role is
# `merge` AND REIFY_RUN_ALL_SKIP_STATE names a state file. Every fixture here
# constructs its own hermetic git repo + closures manifest + state ledger and
# drives run_all.sh with those three keys set; no fixture ever touches real
# repo state.
#
# All facets (SKIP/RUN decision lines, backstop, fail-open storm-escape, inert
# gating, state-ledger update, under-declaration drift-guard) live in this one
# file — one `pool` row in run-all-classification.manifest, shared fixture
# builders, mirroring how test_run_all.sh houses many sub-cases.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RUN_ALL="$SCRIPT_DIR/run_all.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

_TMPDIRS=()
cleanup() { for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done; }
trap cleanup EXIT

echo "=== run_all.sh content-addressed skip engine tests (task 5273) ==="

# ---------------------------------------------------------------------------
# Shared fixture helpers.
# ---------------------------------------------------------------------------

# out_has HAYSTACK NEEDLE / out_lacks HAYSTACK NEEDLE — fork-free bash-native
# substring predicates (mirrors test_run_all.sh's `[[ == *substr* ]]` idiom;
# avoids the grep-under-load flakiness documented at test_run_all.sh:90-98).
out_has()   { [[ "$1" == *"$2"* ]]; }
out_lacks() { [[ "$1" != *"$2"* ]]; }

# git_init_fixture DIR — hermetic throwaway git repo (isolated identity, no
# GPG/hooks). The fixture dir is its own toplevel, so the skip engine's
# `git -C "$INFRA_DIR" rev-parse --show-toplevel` resolves to DIR and its
# repo-relative pathspecs match the files placed at DIR root.
git_init_fixture() {
    local dir="$1"
    git -C "$dir" init -q
    git -C "$dir" config user.email "test@test.com"
    git -C "$dir" config user.name "Test"
    git -C "$dir" config commit.gpgsign false
    git -C "$dir" config core.hooksPath /dev/null
}

# mk_member DIR NAME [RC] — write an executable mock member that exits RC
# (default 0).
mk_member() {
    local dir="$1" name="$2" rc="${3:-0}"
    printf '#!/usr/bin/env bash\nexit %s\n' "$rc" > "$dir/$name"
    chmod +x "$dir/$name"
}

# run_skip STATE CLOSURES DIR [ROLE] — invoke run_all.sh on fixture DIR with
# the content-skip engine keyed active (ROLE default `merge`). Captures
# combined stdout+stderr into the global RUN_OUT and the exit code into
# RUN_RC. Extra `KEY=VALUE` env-prefix tokens may be passed via the global
# RUN_SKIP_ENV array (e.g. backstop-threshold overrides).
run_skip() {
    local state="$1" closures="$2" dir="$3" role="${4:-merge}"
    RUN_RC=0
    RUN_OUT="$(
        DF_VERIFY_ROLE="$role" \
        REIFY_RUN_ALL_CONTENT_SKIP=1 \
        REIFY_RUN_ALL_SKIP_STATE="$state" \
        RUN_ALL_SKIP_CLOSURES_MANIFEST="$closures" \
        "${RUN_SKIP_ENV[@]+${RUN_SKIP_ENV[@]}}" \
        bash "$RUN_ALL" "$dir" 2>&1
    )" || RUN_RC=$?
}
RUN_OUT=""
RUN_RC=0
RUN_SKIP_ENV=()

# ===========================================================================
# Section 1 (step-1): SKIP (content-clean) happy path.
#   A mapped member whose closure is byte-identical between its green sha and
#   HEAD (green_sha == HEAD, clean worktree) must be SKIPPED: a
#   `SKIP (content-clean): <m> green=<sha>` line and NO `--- Running: <m> ---`.
#   RED until the skip engine (step-2) exists.
# ===========================================================================
echo ""
echo "--- Section 1 (step-1): SKIP (content-clean) happy path ---"

S1_DIR="$(mktemp -d)"; _TMPDIRS+=("$S1_DIR")
git_init_fixture "$S1_DIR"
mk_member "$S1_DIR" test_alpha.sh 0
printf 'alpha closure payload\n' > "$S1_DIR/alpha_dep.txt"
git -C "$S1_DIR" add -A
git -C "$S1_DIR" commit -q -m "base"
S1_GREEN="$(git -C "$S1_DIR" rev-parse HEAD)"

# Fixture closures manifest + state ledger live OUTSIDE any declared closure
# (underscore-prefixed, never test_*.sh, created post-commit) so they can
# never be mistaken for a member or a closure-path delta.
S1_CLOSURES="$S1_DIR/_meta_closures.manifest"
printf 'test_alpha.sh alpha_dep.txt\n' > "$S1_CLOSURES"
S1_STATE="$S1_DIR/_meta_state.ledger"
S1_NOW="$(date +%s)"
{
    printf '__MERGES__ 3\n'
    printf 'test_alpha.sh %s %s 3\n' "$S1_GREEN" "$S1_NOW"
} > "$S1_STATE"

run_skip "$S1_STATE" "$S1_CLOSURES" "$S1_DIR"

assert "S1: emits SKIP (content-clean) for the byte-identical mapped member" \
    out_has "$RUN_OUT" "SKIP (content-clean): test_alpha.sh green=$S1_GREEN"

assert "S1: skipped member is NOT executed (no '--- Running: test_alpha.sh ---')" \
    out_lacks "$RUN_OUT" "--- Running: test_alpha.sh ---"

assert "S1: suite still exits 0 when the only member is skipped" \
    test "$RUN_RC" -eq 0

test_summary
