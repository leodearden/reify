#!/usr/bin/env bash
# tests/infra/test_reify_audit_predone_wrapper.sh
#
# Regression guard: asserts that the reify-audit-predone-wrapper.sh script
# exists, is executable, handles --help, and errors appropriately on missing
# required flags — without requiring a live fused-memory MCP server.
#
# Background: the wrapper materializes a TaskMetadata JSON snapshot from the
# fused-memory MCP before invoking reify-audit. This test validates the
# wrapper's basic invocation surface so CI stays GREEN before the systemd
# operator action rewires FUSED_MEMORY_PREDONE_HOOK_REIFY.
#
# See: docs/architecture-audit/f-infra-design.md §11.1
#      task 3731 (root-cause: dead .taskmaster/tasks/tasks.json default)
#
# Auto-discovered by tests/infra/run_all.sh via the test_*.sh glob.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

WRAPPER="$REPO_ROOT/scripts/reify-audit-predone-wrapper.sh"

echo "=== reify-audit-predone-wrapper.sh regression guard ==="

# ==============================================================================
# Check 1: wrapper exists
# ==============================================================================
echo ""
echo "--- Check 1: wrapper script exists ---"

assert "scripts/reify-audit-predone-wrapper.sh exists" \
    bash -c '[ -f "$1" ]' -- "$WRAPPER"

# ==============================================================================
# Check 2: wrapper is executable
# ==============================================================================
echo ""
echo "--- Check 2: wrapper script is executable ---"

assert "scripts/reify-audit-predone-wrapper.sh is executable" \
    bash -c '[ -x "$1" ]' -- "$WRAPPER"

# ==============================================================================
# Check 3: --help exits 0 and prints recognizable usage
# ==============================================================================
echo ""
echo "--- Check 3: wrapper --help exits 0 and mentions key flags ---"

assert "wrapper --help exits 0" \
    bash -c 'bash "$1" --help >/dev/null 2>&1' -- "$WRAPPER"

assert "wrapper --help stdout is non-empty" \
    bash -c '[ -n "$(bash "$1" --help 2>/dev/null)" ]' -- "$WRAPPER"

assert "wrapper --help mentions --task" \
    bash -c 'bash "$1" --help 2>/dev/null | grep -q -- "--task"' -- "$WRAPPER"

assert "wrapper --help mentions --pre-done" \
    bash -c 'bash "$1" --help 2>/dev/null | grep -q -- "--pre-done"' -- "$WRAPPER"

# ==============================================================================
# Check 4: missing --task exits non-zero with usage hint on stderr
# ==============================================================================
echo ""
echo "--- Check 4: wrapper without --task exits non-zero with usage hint ---"

assert "wrapper without --task exits non-zero" \
    bash -c '! bash "$1" 2>/dev/null' -- "$WRAPPER"

assert "wrapper without --task emits usage hint to stderr" \
    bash -c 'bash "$1" 2>&1 >/dev/null | grep -qiE "Usage:|requires --task"' -- "$WRAPPER"

# -- Summary ------------------------------------------------------------------
test_summary
