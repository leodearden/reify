#!/usr/bin/env bash
# Unit tests for scripts/reify-bin-freshness.sh — the shared freshness guard
# library used by the PRD-gate tests (test_prd_gate_corpus.sh,
# test_prd_gate_objective_inheritance.sh) to refuse a verdict against a reify
# binary whose provenance is unproven — e.g. a cross-candidate leftover left
# behind in the shared _merge-verify warm lane by a sibling merge candidate
# (task #5133).
#
# Unlike scripts/reify-audit-freshness.sh (mtime vs. last crate-commit epoch),
# this guard compares a build-time HEAD-SHA sidecar (target/.reify-bin-sha)
# against HEAD at gate time — tree identity, not mtime — because a sibling
# binary built AFTER the candidate's newest source commit would be judged
# "fresh" by an mtime heuristic despite being built from the wrong tree.
#
# Tests:
#   1-2: Script exists and is sourceable
#   3:   reify_bin_is_stale — missing binary → stale (exit 0)
#   4:   reify_bin_is_stale — non-git repo_root → fail-open fresh (exit 1)
#   5:   reify_bin_is_stale — sidecar SHA == HEAD → fresh (exit 1)
#   6:   reify_bin_is_stale — sidecar SHA != HEAD (bogus) → stale (exit 0)
#   7:   reify_bin_is_stale — sidecar absent (git repo_root) → stale (exit 0)
#   8:   reify_bin_stamp — creates target/ + sidecar == HEAD
#   9:   round-trip — stamped repo + present bin → is_stale reports fresh
#   10:  reify_bin_stamp on a non-git dir → non-zero, no sidecar written
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FRESHNESS_LIB="$REPO_ROOT/scripts/reify-bin-freshness.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== reify-bin-freshness.sh unit tests (task #5133) ==="

# Temp fixtures — cleaned up on EXIT.
TMPDIR_BINFRESH=$(mktemp -d /tmp/test-bin-freshness-XXXXXX)
trap 'rm -rf "$TMPDIR_BINFRESH"' EXIT

# A real temp git repo with one commit, so reify_bin_head_sha has a genuine
# HEAD to compare against (mirrors test_reify_audit_freshness.sh's fixture).
GIT_REPO="$TMPDIR_BINFRESH/repo"
mkdir -p "$GIT_REPO"
git -C "$GIT_REPO" init -q
touch "$GIT_REPO/placeholder"
git -C "$GIT_REPO" add placeholder
git -C "$GIT_REPO" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "init" 2>/dev/null
HEAD_SHA=$(git -C "$GIT_REPO" rev-parse HEAD)

# A fake reify binary living under the fixture repo's target/release/.
FAKE_BIN="$GIT_REPO/target/release/reify"
mkdir -p "$(dirname "$FAKE_BIN")"
touch "$FAKE_BIN"
chmod +x "$FAKE_BIN"

# A non-git directory, for the fail-open case. The bin must EXIST here so the
# missing-bin check (which runs first) doesn't mask the fail-open behavior.
NON_GIT_DIR="$TMPDIR_BINFRESH/nongit"
mkdir -p "$NON_GIT_DIR"
NON_GIT_BIN="$NON_GIT_DIR/reify"
touch "$NON_GIT_BIN"
chmod +x "$NON_GIT_BIN"

# ==============================================================================
# Check 1: freshness lib exists
# ==============================================================================
echo ""
echo "--- Check 1: reify-bin-freshness.sh exists ---"

assert "scripts/reify-bin-freshness.sh exists" \
    test -f "$FRESHNESS_LIB"

# ==============================================================================
# Check 2: freshness lib is sourceable
# ==============================================================================
echo ""
echo "--- Check 2: reify-bin-freshness.sh is sourceable ---"

assert "reify-bin-freshness.sh can be sourced without error" \
    bash -c "source '$FRESHNESS_LIB'"

# ==============================================================================
# Check 3: reify_bin_is_stale — missing binary → stale (exit 0)
# ==============================================================================
echo ""
echo "--- Check 3: is_stale returns stale for a missing binary path ---"

assert "is_stale returns stale (exit 0) for a missing binary path" \
    bash -c "source '$FRESHNESS_LIB' && reify_bin_is_stale '$GIT_REPO/target/release/reify-does-not-exist' '$GIT_REPO'"

# ==============================================================================
# Check 4: reify_bin_is_stale — non-git repo_root → fail-open fresh (exit 1)
# ==============================================================================
echo ""
echo "--- Check 4: is_stale fails open (fresh) for a non-git repo_root ---"

assert "is_stale fails open (fresh/exit 1) when repo_root is not a git dir" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_bin_is_stale '$NON_GIT_BIN' '$NON_GIT_DIR'"

# ==============================================================================
# Check 5: reify_bin_is_stale — bin present + sidecar SHA == HEAD → fresh (exit 1)
# ==============================================================================
echo ""
echo "--- Check 5: is_stale returns fresh when the sidecar SHA matches HEAD ---"

mkdir -p "$GIT_REPO/target"
printf '%s\n' "$HEAD_SHA" > "$GIT_REPO/target/.reify-bin-sha"

assert "is_stale returns fresh (exit 1) when sidecar SHA == HEAD" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_bin_is_stale '$FAKE_BIN' '$GIT_REPO'"

# ==============================================================================
# Check 6: reify_bin_is_stale — bin present + sidecar SHA != HEAD → stale (exit 0)
# ==============================================================================
echo ""
echo "--- Check 6: is_stale returns stale when the sidecar SHA does not match HEAD ---"

printf '%s\n' "0000000000000000000000000000000000000000" > "$GIT_REPO/target/.reify-bin-sha"

assert "is_stale returns stale (exit 0) when sidecar SHA != HEAD (bogus SHA)" \
    bash -c "source '$FRESHNESS_LIB' && reify_bin_is_stale '$FAKE_BIN' '$GIT_REPO'"

# ==============================================================================
# Check 7: reify_bin_is_stale — bin present + sidecar absent (git repo) → stale (exit 0)
# ==============================================================================
echo ""
echo "--- Check 7: is_stale returns stale when the sidecar file is absent ---"

rm -f "$GIT_REPO/target/.reify-bin-sha"

assert "is_stale returns stale (exit 0) when the sidecar is absent (unproven provenance)" \
    bash -c "source '$FRESHNESS_LIB' && reify_bin_is_stale '$FAKE_BIN' '$GIT_REPO'"

# ==============================================================================
# Check 8: reify_bin_stamp — creates target/.reify-bin-sha == HEAD (and target/ itself)
# ==============================================================================
echo ""
echo "--- Check 8: reify_bin_stamp creates the sidecar with HEAD's SHA ---"

STAMP_REPO="$TMPDIR_BINFRESH/stamp-repo"
mkdir -p "$STAMP_REPO"
git -C "$STAMP_REPO" init -q
touch "$STAMP_REPO/placeholder"
git -C "$STAMP_REPO" add placeholder
git -C "$STAMP_REPO" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "init" 2>/dev/null
STAMP_HEAD_SHA=$(git -C "$STAMP_REPO" rev-parse HEAD)

# target/ does not exist yet in this fixture — reify_bin_stamp must create it.
assert "target/ does not yet exist in the stamp fixture (precondition)" \
    bash -c "[ ! -d '$STAMP_REPO/target' ]"

assert "reify_bin_stamp succeeds (creates target/ + sidecar)" \
    bash -c "source '$FRESHNESS_LIB' && reify_bin_stamp '$STAMP_REPO'"

assert "reify_bin_stamp creates target/.reify-bin-sha" \
    test -f "$STAMP_REPO/target/.reify-bin-sha"

assert "reify_bin_stamp sidecar contents (trimmed) equal HEAD" \
    bash -c "[ \"\$(cat '$STAMP_REPO/target/.reify-bin-sha')\" = '$STAMP_HEAD_SHA' ]"

# ==============================================================================
# Check 9: round-trip — after stamping, is_stale on a present bin → fresh (exit 1)
# ==============================================================================
echo ""
echo "--- Check 9: round-trip — stamped repo + present bin → fresh ---"

STAMP_BIN="$STAMP_REPO/target/release/reify"
mkdir -p "$(dirname "$STAMP_BIN")"
touch "$STAMP_BIN"
chmod +x "$STAMP_BIN"

assert "is_stale returns fresh (exit 1) after reify_bin_stamp round-trip" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_bin_is_stale '$STAMP_BIN' '$STAMP_REPO'"

# ==============================================================================
# Check 10: reify_bin_stamp on a non-git dir → non-zero, no sidecar written
# ==============================================================================
echo ""
echo "--- Check 10: reify_bin_stamp on a non-git dir fails without writing a sidecar ---"

STAMP_NON_GIT_DIR="$TMPDIR_BINFRESH/stamp-nongit"
mkdir -p "$STAMP_NON_GIT_DIR"

assert "reify_bin_stamp on a non-git dir returns non-zero" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_bin_stamp '$STAMP_NON_GIT_DIR'"

assert "reify_bin_stamp on a non-git dir writes no sidecar" \
    bash -c "[ ! -f '$STAMP_NON_GIT_DIR/target/.reify-bin-sha' ]"

# -- Summary ------------------------------------------------------------------
test_summary
