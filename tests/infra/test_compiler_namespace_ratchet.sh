#!/usr/bin/env bash
# Compiler namespace ratchet (PRD docs/prds/v0_6/compiler-module-namespace.md §4 C7).
# This wrapper is the member run_all.sh discovers (classification_discovered_set
# globs test_*.sh only); the assertions live in the sibling .py. It runs the .py
# outside assert() so the measured count lines reach run_all.sh's output on a
# green run too.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_compiler_namespace_ratchet ==="

assert "python3 is available" command -v python3

_rc=0
python3 "$SCRIPT_DIR/test_compiler_namespace_ratchet.py" || _rc=$?
assert "tests/infra/test_compiler_namespace_ratchet.py exits 0 (rc=$_rc)" test "$_rc" -eq 0

test_summary
