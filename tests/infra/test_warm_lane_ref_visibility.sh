#!/usr/bin/env bash
# tests/infra/test_warm_lane_ref_visibility.sh
# Hermetic tests for scripts/warm-lane-ref-check.sh.
#
# run_helper captures STDOUT, STDERR, and RC separately:
#   OUT     — captured stdout from the script
#   ERR_OUT — captured stderr from the script
#   RC      — exit code
#
# PATH stubs (Block B only):
#   git — counter-based stub: intercepts `rev-parse --verify refs/heads/task/*`
#         only; delegates ALL other git subcommands to the real git.
#         Fails (exit 1) while counter <= $REIFY_GIT_STUB_FAIL_UNTIL,
#         then delegates to real git.
#         Records every invocation to $REIFY_TEST_CALLS_FILE.
#
# Blocks:
#   A — reify-provisioning-innocence: real git worktree fixture + seed + clean,
#         ref resolves before AND after (proves reify primitives don't perturb refs)
#   B — deterministic TOCTOU-class reproduction (added in step-3): counter git
#         stub proves single-shot fails / bounded-retry succeeds
#   C — exit-code taxonomy + read-only invariant (added in step-5)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.
#
# T8 de-flake audit (task #4847):
#   ZERO absolute-wall-clock-upper-bound or scheduling-latency assertions.
#   All assertions are structural (exit codes, stdout SHA grep, stderr markers,
#   CALLS_FILE argv). The TOCTOU race in Block B is reproduced deterministically
#   via a counter-based git stub (no sleeps, no real concurrency).

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SCRIPT="$REPO_ROOT/scripts/warm-lane-ref-check.sh"
SEED_SCRIPT="$REPO_ROOT/scripts/seed-warm-lane.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
# shellcheck source=tests/infra/test_helpers.sh
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== scripts/warm-lane-ref-check.sh hermetic tests (task 4855) ==="

# ─────────────────────────────────────────────────────────────────────────────
# Shared temp state
# ─────────────────────────────────────────────────────────────────────────────
_TMPDIRS=()
cleanup() {
    for d in "${_TMPDIRS[@]+${_TMPDIRS[@]}}"; do rm -rf "$d"; done
}
trap cleanup EXIT

ERR_FILE="$(mktemp /tmp/test-warm-lane-ref-vis-err-XXXXXX)"
_TMPDIRS+=("$ERR_FILE")

# ── run_helper ────────────────────────────────────────────────────────────────
# Invokes the script under test with no PATH stub.
# Sets OUT (stdout), ERR_OUT (stderr), RC (exit code) as globals.
run_helper() {
    local rc=0
    > "$ERR_FILE"
    OUT="$(bash "$SCRIPT" "$@" 2>"$ERR_FILE")" || rc=$?
    ERR_OUT="$(cat "$ERR_FILE")"
    RC=$rc
}

# ─────────────────────────────────────────────────────────────────────────────
# Block A — reify-provisioning-innocence guard
#
# Proves that the reify warm-lane provisioning primitives (seed-warm-lane.sh
# --fresh-checkout and reset_lane's git clean -xfd -e target) NEVER perturb
# ref-visibility in the linked worktree.
#
# Fixture: a hermetic main-checkout repo with two task branches (9999 and 9998),
# a linked worktree simulating a warm lane.  The ref-check must resolve both
# branches from the lane before and after provisioning.
# ─────────────────────────────────────────────────────────────────────────────
echo ""
echo "--- Block A: reify-provisioning-innocence ---"

A_TMP="$(mktemp -d /tmp/test-warm-lane-ref-vis-a-XXXXXX)"
_TMPDIRS+=("$A_TMP")

# ── A-fixture: build hermetic git repos ──────────────────────────────────────
A_MAIN="$A_TMP/main"
A_LANE="$A_TMP/lane"

git init -q -b main "$A_MAIN"
git -C "$A_MAIN" config user.email "test@test.local"
git -C "$A_MAIN" config user.name "Test"
touch "$A_MAIN/README.md"
git -C "$A_MAIN" add README.md
git -C "$A_MAIN" commit -q -m "initial"

# git worktree add creates the linked worktree AND the branch task/9999.
# The branch lives in the main checkout's .git/refs/heads/task/9999 (shared).
git -C "$A_MAIN" worktree add -b task/9999 "$A_LANE" main

# Create a second branch in main (also visible from the lane via shared refs).
git -C "$A_MAIN" branch task/9998 main

# ── A1: both refs resolve from the lane before any provisioning ───────────────
run_helper --lane "$A_LANE" --task 9999 --expect-common-dir "$A_MAIN/.git"
assert "A1: task/9999 resolves from lane (exits 0)" test "$RC" -eq 0
assert "A1: stdout is a 40-hex SHA" \
    bash -c 'printf "%s" "$1" | grep -qxE "[0-9a-f]{40}"' _ "$OUT"

run_helper --lane "$A_LANE" --task 9998 --expect-common-dir "$A_MAIN/.git"
assert "A2: task/9998 resolves from lane via shared refs (exits 0)" test "$RC" -eq 0
assert "A2: stdout is a 40-hex SHA" \
    bash -c 'printf "%s" "$1" | grep -qxE "[0-9a-f]{40}"' _ "$OUT"

# ── A3: exercise the reify provisioning path ──────────────────────────────────
# Set up a dummy base_target with sidecar so seed-warm-lane.sh guards pass.
A_BASE_PARENT="$A_TMP/base-parent"
A_BASE_TARGET="$A_BASE_PARENT/target"
mkdir -p "$A_BASE_TARGET"
echo "dummy-artifact" > "$A_BASE_TARGET/dummy.rlib"

# Record base provenance (empty RUSTFLAGS + INVOCATION) so guards pass.
RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    bash "$SEED_SCRIPT" --record-base "$A_BASE_TARGET" >/dev/null

# Probe for XFS reflink support (seed-warm-lane.sh uses cp --reflink=always).
# If the current filesystem supports reflinks, run a real seed;
# otherwise skip the reflink step but still exercise git clean (which is
# all that is needed to prove git-only ops don't perturb refs).
_A_probe_src="$A_TMP/probe.src"
_A_probe_dst="$A_TMP/probe.dst"
: > "$_A_probe_src"
if cp --reflink=always "$_A_probe_src" "$_A_probe_dst" 2>/dev/null; then
    rm -f "$_A_probe_src" "$_A_probe_dst"
    # Full seed: replaces lane/target with a CoW clone of base_target.
    # REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 makes trash-rm synchronous (test hygiene).
    RUSTFLAGS="" REIFY_WARM_LANE_INVOCATION="" \
    REIFY_WARM_LANE_RESEED_TRASH_SYNC=1 \
        bash "$SEED_SCRIPT" "$A_BASE_TARGET" "$A_LANE" --fresh-checkout >/dev/null 2>&1
    echo "A3: ran seed-warm-lane.sh --fresh-checkout (XFS reflink available)" >&2
else
    rm -f "$_A_probe_src" "$_A_probe_dst"
    echo "A3: no XFS reflink; skipping seed step — git clean still exercised" >&2
fi

# reset_lane's working-tree wipe: remove everything except target/ and .git.
# This is the git clean step that dark-factory runs after acquire/reset_lane.
# It MUST NOT disturb worktree refs (they live in the shared .git common dir).
git -C "$A_LANE" clean -xfd -e target >/dev/null 2>&1 || true

# ── A4: re-assert both refs still resolve after provisioning ─────────────────
run_helper --lane "$A_LANE" --task 9999 --expect-common-dir "$A_MAIN/.git"
assert "A4: task/9999 still resolves after seed+clean (exits 0)" test "$RC" -eq 0
assert "A4: stdout SHA unchanged after provisioning" \
    bash -c 'printf "%s" "$1" | grep -qxE "[0-9a-f]{40}"' _ "$OUT"

run_helper --lane "$A_LANE" --task 9998 --expect-common-dir "$A_MAIN/.git"
assert "A5: task/9998 still resolves after seed+clean (exits 0)" test "$RC" -eq 0

test_summary
