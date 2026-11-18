#!/usr/bin/env bash
# Infrastructure test for task 9993 (cargo_inputs_lib.sh: a verdict never
# depends on the registry cache).
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only. The real assertions live in the sibling .py.
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_cargo_inputs_lib.py (stdlib unittest) exits 0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_cargo_inputs_lib ==="

assert "python3 is available" command -v python3

assert "tests/infra/test_cargo_inputs_lib.py exits 0" \
    python3 "$SCRIPT_DIR/test_cargo_inputs_lib.py"

test_summary
