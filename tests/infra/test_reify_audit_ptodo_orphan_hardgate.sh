#!/usr/bin/env bash
# tests/infra/test_reify_audit_ptodo_orphan_hardgate.sh
#
# Meta-test for task #4733: PTODO orphan hard-gate must run even when the
# reify-audit binary is present-but-stale + REIFY_AUDIT_NO_COLD_BUILD=1.
#
# Proves that test_reify_audit_ptodo.sh's High-severity hard gate (scenarios
# (c) and (d)) is NOT bypassed by the budget-safe skip path when the binary
# is stale.
#
# Design:
#   - Copies the real target/release/reify-audit to a temp path and touches
#     it to an old mtime (year 2000) → a "present-but-stale" binary.
#   - Runs test_reify_audit_ptodo.sh with REIFY_AUDIT_BIN=<stale copy> and
#     REIFY_AUDIT_NO_COLD_BUILD=1, capturing combined stdout+stderr.
#   - Asserts the hard-gate scenarios executed (PASS markers appear in output).
#
# Assertions:
#   (1) The captured output contains the scenario (c-dirty) PASS marker,
#       proving the structural untracked hard gate ran (not whole-script-skipped).
#
# RED today: current test_reify_audit_ptodo.sh exits 0 at the 75-guard before
#            scenario (c) ever prints its PASS line.
# GREEN after step-2 (task #4733): RATCHET_SKIP flag restructure lets
#            scenario (c) execute.
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
PTODO_TEST="$SCRIPT_DIR/test_reify_audit_ptodo.sh"
REAL_BIN="$REPO_ROOT/target/release/reify-audit"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh" >&2
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== PTODO orphan hard-gate meta-test (task #4733) ==="

# Graceful skip when the PTODO test script is absent.
if [ ! -f "$PTODO_TEST" ]; then
    echo "test_reify_audit_ptodo_orphan_hardgate.sh: $PTODO_TEST not found — skipping" >&2
    exit 0
fi

# Graceful skip when the real reify-audit binary is absent.
# (The gate can only be tested with a real binary to copy and make stale.)
if [ ! -x "$REAL_BIN" ]; then
    echo "test_reify_audit_ptodo_orphan_hardgate.sh: $REAL_BIN absent — skipping" >&2
    exit 0
fi

# Graceful skip when required tools are absent.
for _tool in git sqlite3; do
    if ! command -v "$_tool" >/dev/null 2>&1; then
        echo "test_reify_audit_ptodo_orphan_hardgate.sh: $_tool not on PATH — skipping" >&2
        exit 0
    fi
done

# ---------------------------------------------------------------------------
# Setup: copy the real binary to a temp path and touch it to an old mtime.
# A year-2000 timestamp predates all crates/reify-audit commits, so the
# freshness guard always flags it STALE → returns 75 under NO_COLD_BUILD=1.
# ---------------------------------------------------------------------------
OHGM_TMPDIR=$(mktemp -d /tmp/test-ptodo-orphan-hardgate-XXXXXX)
trap 'rm -rf "$OHGM_TMPDIR"' EXIT

STALE_BIN="$OHGM_TMPDIR/reify-audit"
cp "$REAL_BIN" "$STALE_BIN"
touch -t 200001010000 "$STALE_BIN"

# ---------------------------------------------------------------------------
# Invoke test_reify_audit_ptodo.sh under controlled env:
#   REIFY_AUDIT_BIN=<stale copy>   — present-but-stale binary (exists, old mtime)
#   REIFY_AUDIT_NO_COLD_BUILD=1    — arms the budget-safe skip mode
#
# Capture combined stdout+stderr for PASS-marker assertion.
# Use set +e so we can inspect the exit code independently.
# ---------------------------------------------------------------------------
echo ""
echo "--- Invoking test_reify_audit_ptodo.sh with stale binary + NO_COLD_BUILD=1 ---"

OHGM_OUTPUT_FILE="$OHGM_TMPDIR/ptodo-output"
set +e
REIFY_AUDIT_BIN="$STALE_BIN" \
REIFY_AUDIT_NO_COLD_BUILD=1 \
    bash "$PTODO_TEST" >"$OHGM_OUTPUT_FILE" 2>&1
OHGM_EXIT=$?
set -e

echo "test_reify_audit_ptodo.sh exited: $OHGM_EXIT"
echo "--- Captured output (tail) ---"
tail -20 "$OHGM_OUTPUT_FILE"
echo "--- End captured output ---"

# ---------------------------------------------------------------------------
# Assertions
# ---------------------------------------------------------------------------
echo ""
echo "--- Assertions ---"

# (1) The hard-gate scenario (c-dirty) must have EXECUTED.
#     When the current script whole-script-skips at the 75-guard, this PASS
#     line never appears — RED.  After the RATCHET_SKIP restructure (step-2),
#     the stale-present binary lets scenario (c) run and print this marker —
#     GREEN.
assert "(c-dirty) hard gate ran: PASS marker present in output (not whole-script-skipped)" \
    bash -c "grep -qF '(c-dirty) untracked marker' '$OHGM_OUTPUT_FILE'"

test_summary
