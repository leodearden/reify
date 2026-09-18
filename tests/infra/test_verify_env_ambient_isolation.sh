#!/usr/bin/env bash
# Infrastructure test for task 4966 (verify_env ambient isolation), PORTED to
# Python by task 7430 and landed by task 7626.
#
# This wrapper is the member run_all.sh actually discovers: it globs
# `test_*.sh` only (header :22-24, repeated at :1347/:1456/:2013), mirrored by
# classification_discovered_set at run-all-classification-lib.sh:174-183. The
# real assertions live in the sibling .py; without this file they would be
# silently never run, reading as coverage while asserting nothing. Keeping the
# BASENAME is what keeps run-all-classification.manifest:229 and every doc
# reference valid.
#
# This wrapper is also what test_slot_timeout_marker.sh Section F reads through:
# its derivation follows an anchored python3 invocation of a same-stem sibling
# into that sibling, so this member stays in the deadline-capable roster with
# its real route (via:test_occt_flock_gate.sh, which the .py invokes at :520).
#
# Verifies that:
#   1. python3 is on PATH
#   2. tests/infra/test_verify_env_ambient_isolation.py (stdlib unittest)
#      exits 0 -- 26 tests, including one real run of test_occt_flock_gate.sh
#      under the production verify_env ambient

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

[ -f "$SCRIPT_DIR/test_helpers.sh" ] || {
    echo "ERROR: test_helpers.sh not found at $SCRIPT_DIR/test_helpers.sh"
    exit 1
}
source "$SCRIPT_DIR/test_helpers.sh"

echo "=== test_verify_env_ambient_isolation ==="

# ── Preflight ──────────────────────────────────────────────────────────────
assert "python3 is available" command -v python3

# ── The ported suite ──────────────────────────────────────────────────────
assert "tests/infra/test_verify_env_ambient_isolation.py exits 0" \
    python3 "$SCRIPT_DIR/test_verify_env_ambient_isolation.py"

test_summary
