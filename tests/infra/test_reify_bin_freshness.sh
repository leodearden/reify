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
#   11:  resolve_trusted_reify_bin — explicit existing REIFY_BIN → trusted,
#        freshness bypassed entirely
#   12:  resolve_trusted_reify_bin — explicit REIFY_BIN missing path → SKIP,
#        reason mentions REIFY_BIN + missing
#   13:  resolve_trusted_reify_bin — no REIFY_BIN, no target bins → SKIP,
#        reason mentions "not built"
#   14:  resolve_trusted_reify_bin — fresh release bin → trusted, resolves
#        the release path
#   15:  resolve_trusted_reify_bin — fresh debug-only bin → trusted, resolves
#        the debug path
#   16:  resolve_trusted_reify_bin — release bin + stale sidecar → SKIP,
#        reason mentions "stale"
#   17:  resolve_trusted_reify_bin — release+debug both fresh → resolves
#        release (precedence)
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

# ==============================================================================
# Check 11: resolve_trusted_reify_bin — explicit existing REIFY_BIN → trusted
#           (exit 0), REIFY_BIN_RESOLVED == that path, freshness bypassed
#           entirely (repo_root below has NO target/ bins or sidecar at all —
#           if resolve fell through to auto-discovery it would find nothing).
# ==============================================================================
echo ""
echo "--- Check 11: resolve_trusted_reify_bin trusts an explicit existing REIFY_BIN ---"

RESOLVE_EMPTY_REPO="$TMPDIR_BINFRESH/resolve-empty"
mkdir -p "$RESOLVE_EMPTY_REPO"
git -C "$RESOLVE_EMPTY_REPO" init -q
touch "$RESOLVE_EMPTY_REPO/placeholder"
git -C "$RESOLVE_EMPTY_REPO" add placeholder
git -C "$RESOLVE_EMPTY_REPO" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "init" 2>/dev/null

EXPLICIT_BIN="$TMPDIR_BINFRESH/explicit-reify-bin"
touch "$EXPLICIT_BIN"
chmod +x "$EXPLICIT_BIN"

assert "resolve_trusted_reify_bin: explicit existing REIFY_BIN → trusted (exit 0)" \
    env REIFY_BIN="$EXPLICIT_BIN" bash -c "source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO'"

assert "resolve_trusted_reify_bin: explicit REIFY_BIN → REIFY_BIN_RESOLVED equals that path" \
    env REIFY_BIN="$EXPLICIT_BIN" bash -c "source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO' >/dev/null && [ \"\$REIFY_BIN_RESOLVED\" = '$EXPLICIT_BIN' ]"

# ==============================================================================
# Check 12: resolve_trusted_reify_bin — explicit REIFY_BIN pointing at a
#           missing path → SKIP (non-zero), reason mentions REIFY_BIN + missing.
# ==============================================================================
echo ""
echo "--- Check 12: resolve_trusted_reify_bin rejects a missing explicit REIFY_BIN ---"

MISSING_BIN="$TMPDIR_BINFRESH/does-not-exist-reify-bin"

assert "resolve_trusted_reify_bin: missing explicit REIFY_BIN → non-zero (SKIP)" \
    env REIFY_BIN="$MISSING_BIN" bash -c "source '$FRESHNESS_LIB' && ! resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO'"

assert "resolve_trusted_reify_bin: missing explicit REIFY_BIN → reason mentions REIFY_BIN and missing" \
    env REIFY_BIN="$MISSING_BIN" bash -c "source '$FRESHNESS_LIB'; resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO' >/dev/null; echo \"\$REIFY_BIN_SKIP_REASON\" | grep -q 'REIFY_BIN' && echo \"\$REIFY_BIN_SKIP_REASON\" | grep -qi 'missing'"

# ==============================================================================
# Check 13: resolve_trusted_reify_bin — REIFY_BIN unset, no target bins → SKIP,
#           reason mentions 'not built'.
# ==============================================================================
echo ""
echo "--- Check 13: resolve_trusted_reify_bin SKIPs when nothing is built ---"

assert "resolve_trusted_reify_bin: no REIFY_BIN, no target bins → non-zero (SKIP)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && ! resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO'"

assert "resolve_trusted_reify_bin: no REIFY_BIN, no target bins → reason mentions 'not built'" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB'; resolve_trusted_reify_bin '$RESOLVE_EMPTY_REPO' >/dev/null; echo \"\$REIFY_BIN_SKIP_REASON\" | grep -q 'not built'"

# ==============================================================================
# Check 14: resolve_trusted_reify_bin — release bin + sidecar == HEAD → trusted,
#           resolves the release path.
# ==============================================================================
echo ""
echo "--- Check 14: resolve_trusted_reify_bin resolves a fresh release bin ---"

RESOLVE_REPO_D="$TMPDIR_BINFRESH/resolve-d"
mkdir -p "$RESOLVE_REPO_D"
git -C "$RESOLVE_REPO_D" init -q
touch "$RESOLVE_REPO_D/placeholder"
git -C "$RESOLVE_REPO_D" add placeholder
git -C "$RESOLVE_REPO_D" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "init" 2>/dev/null
D_HEAD_SHA=$(git -C "$RESOLVE_REPO_D" rev-parse HEAD)

D_RELEASE_BIN="$RESOLVE_REPO_D/target/release/reify"
mkdir -p "$(dirname "$D_RELEASE_BIN")"
touch "$D_RELEASE_BIN"
chmod +x "$D_RELEASE_BIN"
printf '%s\n' "$D_HEAD_SHA" > "$RESOLVE_REPO_D/target/.reify-bin-sha"

assert "resolve_trusted_reify_bin: fresh release bin → trusted (exit 0)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_D'"

assert "resolve_trusted_reify_bin: fresh release bin → REIFY_BIN_RESOLVED is the release path" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_D' >/dev/null && [ \"\$REIFY_BIN_RESOLVED\" = '$D_RELEASE_BIN' ]"

# ==============================================================================
# Check 15: resolve_trusted_reify_bin — debug bin only + sidecar == HEAD →
#           trusted, resolves the debug path.
# ==============================================================================
echo ""
echo "--- Check 15: resolve_trusted_reify_bin resolves a fresh debug-only bin ---"

RESOLVE_REPO_E="$TMPDIR_BINFRESH/resolve-e"
mkdir -p "$RESOLVE_REPO_E"
git -C "$RESOLVE_REPO_E" init -q
touch "$RESOLVE_REPO_E/placeholder"
git -C "$RESOLVE_REPO_E" add placeholder
git -C "$RESOLVE_REPO_E" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "init" 2>/dev/null
E_HEAD_SHA=$(git -C "$RESOLVE_REPO_E" rev-parse HEAD)

E_DEBUG_BIN="$RESOLVE_REPO_E/target/debug/reify"
mkdir -p "$(dirname "$E_DEBUG_BIN")"
touch "$E_DEBUG_BIN"
chmod +x "$E_DEBUG_BIN"
printf '%s\n' "$E_HEAD_SHA" > "$RESOLVE_REPO_E/target/.reify-bin-sha"

assert "resolve_trusted_reify_bin: fresh debug-only bin → trusted (exit 0)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_E'"

assert "resolve_trusted_reify_bin: fresh debug-only bin → REIFY_BIN_RESOLVED is the debug path" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_E' >/dev/null && [ \"\$REIFY_BIN_RESOLVED\" = '$E_DEBUG_BIN' ]"

# ==============================================================================
# Check 16: resolve_trusted_reify_bin — release bin + sidecar MISMATCH → SKIP,
#           reason mentions 'stale'.
# ==============================================================================
echo ""
echo "--- Check 16: resolve_trusted_reify_bin SKIPs a stale release bin ---"

RESOLVE_REPO_F="$TMPDIR_BINFRESH/resolve-f"
mkdir -p "$RESOLVE_REPO_F"
git -C "$RESOLVE_REPO_F" init -q
touch "$RESOLVE_REPO_F/placeholder"
git -C "$RESOLVE_REPO_F" add placeholder
git -C "$RESOLVE_REPO_F" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "init" 2>/dev/null

F_RELEASE_BIN="$RESOLVE_REPO_F/target/release/reify"
mkdir -p "$(dirname "$F_RELEASE_BIN")"
touch "$F_RELEASE_BIN"
chmod +x "$F_RELEASE_BIN"
printf '%s\n' "0000000000000000000000000000000000000000" > "$RESOLVE_REPO_F/target/.reify-bin-sha"

assert "resolve_trusted_reify_bin: release bin + sidecar mismatch → non-zero (SKIP)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && ! resolve_trusted_reify_bin '$RESOLVE_REPO_F'"

assert "resolve_trusted_reify_bin: release bin + sidecar mismatch → reason mentions 'stale'" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB'; resolve_trusted_reify_bin '$RESOLVE_REPO_F' >/dev/null; echo \"\$REIFY_BIN_SKIP_REASON\" | grep -qi 'stale'"

# ==============================================================================
# Check 17: resolve_trusted_reify_bin — both release+debug present and fresh →
#           resolves release (precedence).
# ==============================================================================
echo ""
echo "--- Check 17: resolve_trusted_reify_bin prefers release over debug ---"

RESOLVE_REPO_G="$TMPDIR_BINFRESH/resolve-g"
mkdir -p "$RESOLVE_REPO_G"
git -C "$RESOLVE_REPO_G" init -q
touch "$RESOLVE_REPO_G/placeholder"
git -C "$RESOLVE_REPO_G" add placeholder
git -C "$RESOLVE_REPO_G" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "init" 2>/dev/null
G_HEAD_SHA=$(git -C "$RESOLVE_REPO_G" rev-parse HEAD)

G_RELEASE_BIN="$RESOLVE_REPO_G/target/release/reify"
G_DEBUG_BIN="$RESOLVE_REPO_G/target/debug/reify"
mkdir -p "$(dirname "$G_RELEASE_BIN")" "$(dirname "$G_DEBUG_BIN")"
touch "$G_RELEASE_BIN" "$G_DEBUG_BIN"
chmod +x "$G_RELEASE_BIN" "$G_DEBUG_BIN"
printf '%s\n' "$G_HEAD_SHA" > "$RESOLVE_REPO_G/target/.reify-bin-sha"

assert "resolve_trusted_reify_bin: both bins present+fresh → trusted (exit 0)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_G'"

assert "resolve_trusted_reify_bin: both bins present+fresh → resolves release (precedence over debug)" \
    bash -c "unset REIFY_BIN; source '$FRESHNESS_LIB' && resolve_trusted_reify_bin '$RESOLVE_REPO_G' >/dev/null && [ \"\$REIFY_BIN_RESOLVED\" = '$G_RELEASE_BIN' ]"

# -- Summary ------------------------------------------------------------------
test_summary
