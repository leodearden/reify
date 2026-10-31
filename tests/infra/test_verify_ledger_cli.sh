#!/usr/bin/env bash
# Infrastructure test for task 7424 (verify instrumentation ledger CLI).
#
# This wrapper is the member run_all.sh actually discovers (it globs
# `test_*.sh` only); the assertions live in the sibling .py, which drives
# scripts/verify-ledger.py as a subprocess CLI.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_ledger_cli.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_ledger_cli ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_verify_ledger_cli.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_ledger_cli.py"

test_summary
