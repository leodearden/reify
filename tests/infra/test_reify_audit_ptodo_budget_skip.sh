#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo_budget_skip.sh
#
# Meta-test for task #4624: budget-safe PTODO skip path.
#
# Invokes tests/infra/test_reify_audit_ptodo.sh as a subprocess under a
# controlled environment to prove that REIFY_AUDIT_NO_COLD_BUILD=1 causes the
# script to exit 0 (graceful SKIP) WITHOUT invoking `cargo build`.
#
# Design:
#   - A shim `cargo` on PATH writes a marker file when invoked.
#   - REIFY_AUDIT_BIN is set to a nonexistent path so the freshness guard
#     always sees an absent/stale binary.
#   - REIFY_AUDIT_NO_COLD_BUILD=1 arms the budget-safe skip mode.
#
# Assertions:
#   (1) test_reify_audit_ptodo.sh exits 0 (graceful SKIP, not a test failure)
#   (2) its combined output contains a skip message
#   (3) the shim cargo marker was NOT created (no cold build attempted)
#
# RED: (3) fails today because test_reify_audit_ptodo.sh hardcodes the bin path
#      (REIFY_AUDIT_BIN override ignored), uses rebuild not rebuild-budget-safe,
#      and its explicit fallback `cargo build` fires regardless of
#      REIFY_AUDIT_NO_COLD_BUILD — so the shim IS invoked and the marker IS created.
# GREEN: after impl-ptodo-skip, all three assertions pass.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PTODO_TEST="$SCRIPT_DIR/test_reify_audit_ptodo.sh"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== PTODO budget-safe skip meta-test (task #4624) ==="

# Graceful skip when bash or the PTODO test script are absent.
if [ ! -f "$PTODO_TEST" ]; then
    echo "test_reify_audit_ptodo_budget_skip.sh: $PTODO_TEST not found — skipping" >&2
    exit 0
fi

# ---------------------------------------------------------------------------
# Setup: temp dir with shim cargo that writes a marker file when invoked.
# The shim exits 0 (like a real cargo) but never creates any binary — so the
# freshness guard will always re-see an absent binary after the shim runs.
# ---------------------------------------------------------------------------
BS_META_TMPDIR=$(mktemp -d /tmp/test-ptodo-budget-skip-XXXXXX)
trap 'rm -rf "$BS_META_TMPDIR"' EXIT

BS_MARKER="$BS_META_TMPDIR/cargo-was-invoked"
FAKE_BIN_PATH="$BS_META_TMPDIR/nonexistent-reify-audit-$$"

# Shim cargo: writes marker to prove invocation, then exits 0.
cat > "$BS_META_TMPDIR/cargo" <<EOF
#!/usr/bin/env bash
# Shim cargo for budget-safe skip meta-test — writes marker and exits 0.
touch '$BS_MARKER'
exit 0
EOF
chmod +x "$BS_META_TMPDIR/cargo"

# ---------------------------------------------------------------------------
# Invoke test_reify_audit_ptodo.sh under controlled env:
#   REIFY_AUDIT_BIN=<nonexistent>  — overrides the binary path (after impl)
#   REIFY_AUDIT_NO_COLD_BUILD=1    — arms the budget-safe skip
#   PATH=<shimdir>:$PATH           — shim cargo intercepts any cargo invocation
#
# Capture combined stdout+stderr for skip-message assertion.
# Use set +e so we can inspect the exit code independently.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_ptodo.sh under budget-safe env ---"

BS_OUTPUT_FILE="$BS_META_TMPDIR/ptodo-output"
set +e
REIFY_AUDIT_BIN="$FAKE_BIN_PATH" \
REIFY_AUDIT_NO_COLD_BUILD=1 \
PATH="$BS_META_TMPDIR:$PATH" \
    bash "$PTODO_TEST" >"$BS_OUTPUT_FILE" 2>&1
BS_EXIT=$?
set -e

# ---------------------------------------------------------------------------
# Assertions
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertions ---"

# (1) Script must exit 0 (graceful SKIP, not a test failure or infra error).
assert "test_reify_audit_ptodo.sh exits 0 (graceful SKIP) under REIFY_AUDIT_NO_COLD_BUILD=1" \
    bash -c "[ '$BS_EXIT' -eq 0 ]"

# (2) Combined output must contain a skip message.
assert "output contains a skip message (e.g. 'skip' or 'SKIP')" \
    bash -c "grep -qi 'skip' '$BS_OUTPUT_FILE'"

# (3) Shim cargo must NOT have been invoked — marker file must be absent.
#     A present marker proves a cold build was attempted, violating the budget-safe contract.
assert "shim cargo NOT invoked — no cold build attempted (marker file absent)" \
    bash -c "[ ! -f '$BS_MARKER' ]"

test_summary
