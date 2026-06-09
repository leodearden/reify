#!/usr/bin/env bash
# Unit tests for scripts/reify-audit-freshness.sh — the shared freshness
# guard library that routes both the predone wrapper and the /audit skill
# through a single staleness check.
#
# Tests:
#   1-2: Script exists and is sourceable
#   3:   reify_audit_crate_commit_epoch prints a positive integer
#   4-7: reify_audit_is_stale: stale bin, fresh bin, missing bin, non-git repo
#   8-9: reify_audit_guard refuse-mode: stale exits non-zero with message,
#        fresh exits 0 silently
#  10:   reify_audit_guard rebuild-mode: fake cargo that touches bin → exit 0
#  11:   reify_audit_guard rebuild-mode: fake cargo that does NOT freshen → non-zero
#  12:   is_stale warns (stderr) when inside a git repo with no crates/reify-audit
#        history — fail-open (fresh) but not silent (likely renamed crate path)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
FRESHNESS_LIB="$REPO_ROOT/scripts/reify-audit-freshness.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

# Temp dir for fake binaries — cleaned up on EXIT.
TMPDIR_FRESHNESS=$(mktemp -d /tmp/test-freshness-XXXXXX)
trap 'rm -rf "$TMPDIR_FRESHNESS"' EXIT

echo "=== reify-audit-freshness.sh unit tests ==="

# ==============================================================================
# Check 1: freshness lib exists
# ==============================================================================
echo ""
echo "--- Check 1: reify-audit-freshness.sh exists ---"

assert "scripts/reify-audit-freshness.sh exists" \
    test -f "$FRESHNESS_LIB"

# ==============================================================================
# Check 2: freshness lib is sourceable
# ==============================================================================
echo ""
echo "--- Check 2: reify-audit-freshness.sh is sourceable ---"

assert "reify-audit-freshness.sh can be sourced without error" \
    bash -c "source '$FRESHNESS_LIB'"

# ==============================================================================
# Check 3: reify_audit_crate_commit_epoch prints a positive integer
# ==============================================================================
echo ""
echo "--- Check 3: reify_audit_crate_commit_epoch prints positive integer ---"

assert "reify_audit_crate_commit_epoch prints a positive integer epoch" \
    bash -c "source '$FRESHNESS_LIB' && epoch=\$(reify_audit_crate_commit_epoch '$REPO_ROOT') && [[ \"\$epoch\" =~ ^[0-9]+\$ ]] && [ \"\$epoch\" -gt 0 ]"

# ==============================================================================
# Check 4: reify_audit_is_stale — bin touched to old epoch → stale (exit 0)
# ==============================================================================
echo ""
echo "--- Check 4: is_stale returns stale for an old binary ---"

# Create a fake binary touched to 2000-01-01 00:00 (epoch 946684800).
STALE_BIN="$TMPDIR_FRESHNESS/reify-audit-stale"
touch "$STALE_BIN"
touch -t 200001010000 "$STALE_BIN"

assert "is_stale returns stale (exit 0) for a binary older than crate commit" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_is_stale '$STALE_BIN' '$REPO_ROOT'"

# ==============================================================================
# Check 5: reify_audit_is_stale — bin touched to now → fresh (exit 1)
# ==============================================================================
echo ""
echo "--- Check 5: is_stale returns fresh for a current binary ---"

FRESH_BIN="$TMPDIR_FRESHNESS/reify-audit-fresh"
touch "$FRESH_BIN"
# File was just created — mtime is now, which is after any historical commit.

assert "is_stale returns fresh (exit 1) for a binary newer than crate commit" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_audit_is_stale '$FRESH_BIN' '$REPO_ROOT'"

# ==============================================================================
# Check 6: reify_audit_is_stale — missing bin → stale (exit 0)
# ==============================================================================
echo ""
echo "--- Check 6: is_stale returns stale for a missing binary ---"

assert "is_stale returns stale (exit 0) for a missing binary path" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_is_stale '/tmp/nonexistent-reify-audit-$$' '$REPO_ROOT'"

# ==============================================================================
# Check 7: reify_audit_is_stale — non-git repo_root → fail-open (fresh, exit 1)
# ==============================================================================
echo ""
echo "--- Check 7: is_stale fails open (fresh) for undeterminable epoch ---"

NON_GIT_DIR=$(mktemp -d /tmp/test-nongit-XXXXXX)
trap 'rm -rf "$TMPDIR_FRESHNESS" "$NON_GIT_DIR"' EXIT
touch "$NON_GIT_DIR/fake-bin"

assert "is_stale fails open (returns fresh/exit 1) when repo_root is not a git dir" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_audit_is_stale '$NON_GIT_DIR/fake-bin' '$NON_GIT_DIR'"

# ==============================================================================
# Check 8: reify_audit_guard refuse — stale bin → exits non-zero + stderr message
# ==============================================================================
echo ""
echo "--- Check 8: guard refuse-mode exits non-zero and prints stale message ---"

assert "guard refuse-mode: stale binary exits non-zero" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_audit_guard '$STALE_BIN' refuse '$REPO_ROOT' 2>/dev/null"

assert "guard refuse-mode: stale binary stderr contains 'stale'" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$STALE_BIN' refuse '$REPO_ROOT' 2>&1 | grep -qi 'stale'"

assert "guard refuse-mode: stale binary stderr contains 'cargo install'" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$STALE_BIN' refuse '$REPO_ROOT' 2>&1 | grep -q 'cargo install'"

# Exit code must be 125 specifically.
set +e
(source "$FRESHNESS_LIB" && reify_audit_guard "$STALE_BIN" refuse "$REPO_ROOT") 2>/dev/null
GUARD_EXIT=$?
set -e
assert "guard refuse-mode: stale binary exits with code 125" \
    bash -c "[ '$GUARD_EXIT' -eq 125 ]"

# ==============================================================================
# Check 9: reify_audit_guard refuse — fresh bin → exits 0 silently
# ==============================================================================
echo ""
echo "--- Check 9: guard refuse-mode exits 0 silently for a fresh binary ---"

assert "guard refuse-mode: fresh binary exits 0" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$FRESH_BIN' refuse '$REPO_ROOT' 2>/dev/null"

# ==============================================================================
# Check 10: reify_audit_guard rebuild — fake cargo that freshens bin → exit 0
# ==============================================================================
echo ""
echo "--- Check 10: guard rebuild-mode succeeds when fake cargo freshens the bin ---"

# The rebuild branch calls `cargo build --release -q -p reify-audit` inside
# REPO_ROOT. We shim cargo with a script that touches the test bin (making
# it fresh), then exits 0. The guard should re-check is_stale and return 0.
REBUILD_TMPDIR=$(mktemp -d /tmp/test-rebuild-XXXXXX)
trap 'rm -rf "$TMPDIR_FRESHNESS" "$NON_GIT_DIR" "$REBUILD_TMPDIR"' EXIT

# Stale bin for rebuild test
REBUILD_BIN="$TMPDIR_FRESHNESS/reify-audit-for-rebuild"
touch "$REBUILD_BIN"
touch -t 200001010000 "$REBUILD_BIN"

# Fake cargo: touch the rebuild bin (making it fresh) and exit 0
FAKE_CARGO_TOUCH="$REBUILD_TMPDIR/cargo"
cat > "$FAKE_CARGO_TOUCH" <<EOF
#!/usr/bin/env bash
# Fake cargo for rebuild test — freshens REBUILD_BIN
touch '$REBUILD_BIN'
exit 0
EOF
chmod +x "$FAKE_CARGO_TOUCH"

assert "guard rebuild-mode: fake cargo that freshens bin → exit 0" \
    env PATH="$REBUILD_TMPDIR:$PATH" bash -c "source '$FRESHNESS_LIB' && reify_audit_guard '$REBUILD_BIN' rebuild '$REPO_ROOT' 2>/dev/null"

# ==============================================================================
# Check 11: reify_audit_guard rebuild — fake cargo that does NOT freshen → non-zero
# ==============================================================================
echo ""
echo "--- Check 11: guard rebuild-mode fails when fake cargo does NOT freshen bin ---"

# Re-stale the bin
STUBBORN_BIN="$TMPDIR_FRESHNESS/reify-audit-stubborn"
touch "$STUBBORN_BIN"
touch -t 200001010000 "$STUBBORN_BIN"

# Fake cargo: exits 0 but never touches the bin
FAKE_CARGO_NOOP="$REBUILD_TMPDIR/cargo-noop"
cat > "$FAKE_CARGO_NOOP" <<'EOF'
#!/usr/bin/env bash
# Fake cargo for rebuild test — exits 0 but does NOT freshen the bin
exit 0
EOF
chmod +x "$FAKE_CARGO_NOOP"

# Swap in the no-op fake cargo (replace cargo symlink in REBUILD_TMPDIR)
mv "$FAKE_CARGO_TOUCH" "$REBUILD_TMPDIR/cargo-touch-bak"
cp "$FAKE_CARGO_NOOP" "$REBUILD_TMPDIR/cargo"
chmod +x "$REBUILD_TMPDIR/cargo"

assert "guard rebuild-mode: fake cargo that does NOT freshen bin → exits non-zero" \
    env PATH="$REBUILD_TMPDIR:$PATH" bash -c "source '$FRESHNESS_LIB' && ! reify_audit_guard '$STUBBORN_BIN' rebuild '$REPO_ROOT' 2>/dev/null"

# ==============================================================================
# Check 12: reify_audit_is_stale — git repo with no crates/reify-audit history
#           → fail-open (fresh, exit 1) AND emits a stderr warning
#           This exercises the guard's renamed-crate-path detection (suggestion 3):
#           a non-git dir is legitimately silent; a git tree with no such history
#           is likely a misconfiguration and must warn so the silent disable is
#           visible.
# ==============================================================================
echo ""
echo "--- Check 12: is_stale warns when git repo has no crates/reify-audit history ---"

# Create a minimal git repo that has no crates/reify-audit history at all.
GIT_NO_HIST_DIR=$(mktemp -d /tmp/test-git-nohist-XXXXXX)
trap 'rm -rf "$TMPDIR_FRESHNESS" "$NON_GIT_DIR" "$REBUILD_TMPDIR" "$GIT_NO_HIST_DIR"' EXIT
git -C "$GIT_NO_HIST_DIR" init -q
touch "$GIT_NO_HIST_DIR/placeholder"
git -C "$GIT_NO_HIST_DIR" add placeholder
git -C "$GIT_NO_HIST_DIR" \
    -c user.name="Test" \
    -c user.email="test@test.com" \
    commit -qm "init" 2>/dev/null
touch "$GIT_NO_HIST_DIR/fake-bin"

# 12a: Still returns fresh (fail-open) — guard must not block in this case.
assert "is_stale fails open (fresh/exit 1) in git repo with no crates/reify-audit history" \
    bash -c "source '$FRESHNESS_LIB' && ! reify_audit_is_stale '$GIT_NO_HIST_DIR/fake-bin' '$GIT_NO_HIST_DIR' 2>/dev/null"

# 12b: But emits a warning to stderr (not silent like the non-git case).
# Pattern: source freshness lib, then run is_stale with stderr→stdout, pipe to grep.
# Pipeline exit code = grep's exit code (0 if warning found).
assert "is_stale emits a stderr warning in git repo with no crates/reify-audit history" \
    bash -c "source '$FRESHNESS_LIB' && reify_audit_is_stale '$GIT_NO_HIST_DIR/fake-bin' '$GIT_NO_HIST_DIR' 2>&1 | grep -qi 'crates/reify-audit'"

# 12c: Non-git dir (Check 7) is still silent — confirm no regression.
# Capture stderr from is_stale; a silent path leaves the var empty.
assert "is_stale is silent (no warning) for a non-git repo_root" \
    bash -c "source '$FRESHNESS_LIB'; warn=\$(reify_audit_is_stale '$NON_GIT_DIR/fake-bin' '$NON_GIT_DIR' 2>&1 >/dev/null); [ -z \"\$warn\" ]"

# -- Summary ------------------------------------------------------------------
test_summary
