#!/usr/bin/env bash
# Trampoline-decode and result-build construct ratchets (PRD
# docs/prds/v0_6/trampoline-decode-and-result-build.md §4 C6). This wrapper is
# the member run_all.sh discovers (classification_discovered_set globs test_*.sh
# only); the assertions live in the sibling .py. It runs the .py outside
# assert() so the measured row counts reach run_all.sh's output on a green run
# too.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_trampoline_decode_ratchet ==="

assert "python3 is available" command -v python3

_rc=0
python3 "$SCRIPT_DIR/test_trampoline_decode_ratchet.py" || _rc=$?
assert "tests/infra/test_trampoline_decode_ratchet.py exits 0 (rc=$_rc)" test "$_rc" -eq 0

test_summary
