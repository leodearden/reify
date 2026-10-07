#!/usr/bin/env bash
# Infrastructure test for task 6932 (PPRDSTATUS escalation wiring).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only. The real assertions live in the sibling .py; without this
# file they would be silently never run, reading as coverage while asserting
# nothing.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_reify_audit_pprdstatus_wiring.py (stdlib unittest) exits 0
#   3. scripts/pprdstatus-escalate.py --help exits 0 (CLI smoke)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_reify_audit_pprdstatus_wiring ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_reify_audit_pprdstatus_wiring.py exits 0" \
    python3 "$SCRIPT_DIR/test_reify_audit_pprdstatus_wiring.py"

# ── CLI smoke ─────────────────────────────────────────────────────────────
assert "scripts/pprdstatus-escalate.py --help exits 0" \
    python3 "$ROOT/scripts/pprdstatus-escalate.py" --help

test_summary
