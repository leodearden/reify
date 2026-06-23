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
#   - Asserts the hard-gate scenarios executed via stable machine-readable
#     sentinels emitted by test_reify_audit_ptodo.sh only when all scenario
#     asserts pass.
#
# Assertions:
#   (1) @@HARDGATE_C_PASSED@@ emitted → scenario (c) ran and all (c) asserts
#       passed (hard gate was NOT bypassed by the stale-binary skip path).
#   (2) @@HARDGATE_D_PASSED@@ emitted → scenario (d) orphaned-cite gate ran
#       and all (d) asserts passed (exact incident class from 2026-06-22/23).
#   (3) test_reify_audit_ptodo.sh exited 0 (defense-in-depth: a gate break
#       that makes the underlying test fail would be caught here independently
#       of the sentinel check).
#
# Sentinel contract (decoupled from prose):
#   - @@HARDGATE_C_PASSED@@ is echoed ONLY when the FAIL counter is unchanged
#     across all (c) asserts → broken gate suppresses the sentinel (RED).
#   - @@HARDGATE_D_PASSED@@ is echoed ONLY when the FAIL counter is unchanged
#     across all (d) asserts → same property.
#   - These tokens contain no TODO/FIXME/HACK substring and appear only in
#     echo lines, so they do not trip the repo's own PTODO self-sweep.
#
# RED today (step-5): test_reify_audit_ptodo.sh does not yet emit the
#   sentinels, so both sentinel greps fail and this meta-test exits 1.
# GREEN after step-6 (task #4733): sentinels are emitted on the passing branch.
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

# (1) Scenario (c) all-asserts-passed sentinel.
#     @@HARDGATE_C_PASSED@@ is echoed by test_reify_audit_ptodo.sh ONLY when the
#     FAIL counter is unchanged across all (c) asserts (i.e. the gate ran and
#     every (c) assert passed).  When the old code whole-script-skipped at the
#     75-guard, this token never appeared — RED.  After the RATCHET_SKIP
#     restructure (step-2) + sentinel emission (step-6), it appears — GREEN.
assert "scenario (c) sentinel @@HARDGATE_C_PASSED@@ emitted (hard gate ran and all (c) asserts passed)" \
    bash -c "grep -qF '@@HARDGATE_C_PASSED@@' '$OHGM_OUTPUT_FILE'"

# (2) Scenario (d) all-asserts-passed sentinel.
#     @@HARDGATE_D_PASSED@@ is echoed ONLY when all (d) orphaned-cite asserts pass.
#     RED until step-6 emits the sentinel from test_reify_audit_ptodo.sh.
assert "scenario (d) sentinel @@HARDGATE_D_PASSED@@ emitted (orphaned-cite gate ran and all (d) asserts passed)" \
    bash -c "grep -qF '@@HARDGATE_D_PASSED@@' '$OHGM_OUTPUT_FILE'"

# (3) Defense-in-depth: the underlying test must have exited 0.
#     A gate break that makes test_reify_audit_ptodo.sh exit 1 is caught here
#     independently of the sentinel check (sentinels are suppressed on failure
#     anyway, but this provides a second independent guard).
assert "test_reify_audit_ptodo.sh exited 0 (underlying test did not fail)" \
    bash -c '[ "$1" -eq 0 ]' -- "$OHGM_EXIT"

test_summary
