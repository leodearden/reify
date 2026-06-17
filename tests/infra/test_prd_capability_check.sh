#!/usr/bin/env bash
# Infrastructure test for task 4607 (prd-gate-exec α — capability probe runner).
# Verifies that:
#   1. python3 is on PATH
#   2. scripts/test_prd_capability_check.py (stdlib unittest) exits 0
#   3. scripts/prd-capability-check.py --help exits 0 (CLI smoke)
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

echo "=== test_prd_capability_check ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "scripts/test_prd_capability_check.py exits 0" \
    python3 "$REPO_ROOT/scripts/test_prd_capability_check.py"

# ── CLI smoke ─────────────────────────────────────────────────────────────
assert "scripts/prd-capability-check.py --help exits 0" \
    python3 "$REPO_ROOT/scripts/prd-capability-check.py" --help

test_summary
