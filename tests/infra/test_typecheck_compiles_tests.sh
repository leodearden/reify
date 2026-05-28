#!/usr/bin/env bash
# Infrastructure drift test for task 4042.
# Asserts that scripts/verify.sh's typecheck gate compiles integration tests
# by passing `--tests` to `cargo check --workspace`.
#
# Without `--tests`, cargo does not compile test targets (integration tests
# under tests/, doctests), so test-binary breakage can slip through the gate —
# exactly the failure mode that allowed η's broken test imports to merge.
#
# RED state: verify.sh emits plain `cargo check --workspace` (no --tests).
# GREEN state: verify.sh emits `cargo check --workspace --tests`.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || { echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"; exit 1; }
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== typecheck gate compiles integration tests (--tests flag) ==="

# Capture the typecheck plan (strip comment lines).
# --scope all forces the full plan regardless of changed-file scope.
TYPECHECK_PLAN_SEGS="$(bash "$REPO_ROOT/scripts/verify.sh" typecheck --scope all --print-plan | grep -v '^#')"
export TYPECHECK_PLAN_SEGS

# -- Test 1: typecheck plan contains cargo check --workspace --tests ------------
echo ""
echo "--- Test 1: typecheck plan includes --tests flag on cargo check ---"

assert "plan contains 'cargo check --workspace … --tests'" \
    bash -c "printf '%s\n' \"\$TYPECHECK_PLAN_SEGS\" | grep -qE 'cargo check --workspace.*--tests'"

# -- Test 2: no bare cargo check --workspace without --tests --------------------
echo ""
echo "--- Test 2: no bare 'cargo check --workspace' without --tests (regression guard) ---"

# This is the regression we are preventing: a bare cargo check --workspace
# silently skips test targets, letting broken test-binary imports merge.
# Guard also requires ≥1 'cargo check --workspace' line so the assertion
# cannot pass spuriously when the gate disappears entirely (e.g. RUN_RUST=0):
# without the leading grep -qE check, an empty plan would produce no lines for
# the pipeline, leaving grep -qEv with no input (exit 1) and the leading !
# would flip that to 0 — a false PASS.
assert "plan does NOT contain a bare 'cargo check --workspace' without --tests" \
    bash -c "printf '%s\n' \"\$TYPECHECK_PLAN_SEGS\" | grep -qE 'cargo check --workspace' && ! printf '%s\n' \"\$TYPECHECK_PLAN_SEGS\" | grep -E 'cargo check --workspace' | grep -qEv 'cargo check --workspace.*--tests'"

test_summary
