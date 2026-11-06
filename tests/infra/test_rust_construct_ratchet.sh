#!/usr/bin/env bash
# Contract tests for the construct-ratchet engine tests/infra/rust_construct_ratchet.py.
# This wrapper is the member run_all.sh discovers (classification_discovered_set
# globs test_*.sh only); the assertions live in the sibling .py.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_rust_construct_ratchet ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_rust_construct_ratchet.py exits 0" \
    python3 "$SCRIPT_DIR/test_rust_construct_ratchet.py"

test_summary
