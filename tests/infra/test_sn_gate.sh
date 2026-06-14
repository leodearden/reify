#!/usr/bin/env bash
# Infrastructure test for task 4455 (A′ s(N) gate).
# Verifies that:
#   1. python3 is on PATH
#   2. scripts/test_sn_gate.py (stdlib unittest) exits 0
#   3. scripts/sn_gate.py --help exits 0 (CLI smoke)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_sn_gate ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "scripts/test_sn_gate.py exits 0" \
    python3 "$ROOT/scripts/test_sn_gate.py"

# ── CLI smoke ─────────────────────────────────────────────────────────────
assert "scripts/sn_gate.py --help exits 0" \
    python3 "$ROOT/scripts/sn_gate.py" --help

test_summary
