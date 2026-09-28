#!/usr/bin/env bash
# Infrastructure test for task 7899 (self-referential fd-probe guard).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py. Rationale and
# scope: tests/infra/README.md "Self-referential fd-probe guard".

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_fd_probe_self_reference ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── Unit tests ────────────────────────────────────────────────────────────
assert "tests/infra/test_fd_probe_self_reference.py exits 0" \
    python3 "$SCRIPT_DIR/test_fd_probe_self_reference.py"

# ── CLI smoke ─────────────────────────────────────────────────────────────
assert "scripts/check-fd-probe-self-reference.py --help exits 0" \
    python3 "$ROOT/scripts/check-fd-probe-self-reference.py" --help

test_summary
